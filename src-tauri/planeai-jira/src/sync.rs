use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::client::JiraClient;
use crate::config::JiraConfig;
use crate::repository::JiraRepository;
use crate::writeback::{JiraWriteback, WritebackAction};
use planeai_tasks::model::{CreateParams, ListFilter, Status, UpdateParams};
use planeai_tasks::provider::TaskProvider;

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct SyncResult {
    pub created: usize,
    pub updated: usize,
    pub done: usize,
    pub errors: usize,
}

pub struct JiraSync {
    client: Arc<JiraClient>,
    repo: Arc<JiraRepository>,
    task_provider: Arc<dyn TaskProvider + Send + Sync>,
    config: JiraConfig,
    writeback: JiraWriteback,
}

impl JiraSync {
    pub fn new(
        client: Arc<JiraClient>,
        repo: Arc<JiraRepository>,
        task_provider: Arc<dyn TaskProvider + Send + Sync>,
        config: JiraConfig,
    ) -> Self {
        let writeback = JiraWriteback::new(Arc::clone(&client));
        Self {
            client,
            repo,
            task_provider,
            config,
            writeback,
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
                        Ok(r) => info!(created = r.created, updated = r.updated, done = r.done, errors = r.errors, "jira sync complete"),
                        Err(e) => warn!(error = %e, "jira sync error"),
                    }
                }
            }
        }
    }

    pub async fn sync_now(&self) -> Result<SyncResult, crate::Error> {
        let mut result = SyncResult::default();

        if self.config.projects.is_empty() {
            tracing::warn!("sync_now: no project mappings configured");
            return Ok(result);
        }

        for (source_name, mapping) in &self.config.projects {
            match self.sync_project(source_name, mapping, &mut result).await {
                Ok(()) => {}
                Err(e) => {
                    warn!(project = %mapping.jira_project, error = %e, "sync failed for project, continuing");
                    result.errors += 1;
                }
            }
        }

        Ok(result)
    }

    async fn sync_project(
        &self,
        source_name: &str,
        mapping: &crate::config::JiraProjectMapping,
        result: &mut SyncResult,
    ) -> Result<(), crate::Error> {
        let issues = self.client.search(&mapping.jql).await?;

        let mut seen_keys = HashSet::new();

        for issue in &issues {
            seen_keys.insert(issue.issue_key.clone());

            // Upsert raw issue into local store
            let jira_issue = crate::model::JiraIssue {
                issue_key: issue.issue_key.clone(),
                jira_project: mapping.jira_project.clone(),
                summary: issue.summary.clone(),
                description: issue.description.clone(),
                status: issue.status.clone(),
                priority: issue.priority.clone(),
                labels: issue.labels.clone(),
                sync_status: crate::model::SyncStatus::Synced,
                last_synced_at: chrono::Utc::now(),
                source_name: source_name.to_string(),
            };
            self.repo.upsert_issue(&jira_issue)?;

            match self.task_provider.get(&issue.issue_key) {
                Err(planeai_tasks::provider::Error::NotFound) => {
                    let status = map_status(&issue.status, &mapping.status_map);
                    let priority = map_priority(issue.priority.as_deref());
                    self.task_provider.create(CreateParams {
                        key: Some(issue.issue_key.clone()),
                        title: issue.summary.clone(),
                        description: issue.description.clone(),
                        status: Some(status),
                        priority,
                        tags: issue.labels.clone(),
                        ..Default::default()
                    })?;
                    result.created += 1;
                }
                Ok(task) => {
                    let new_status = map_status(&issue.status, &mapping.status_map);
                    let needs_update = task.title != issue.summary
                        || task.description != issue.description
                        || task.status != new_status;

                    if needs_update {
                        self.task_provider.update(
                            &issue.issue_key,
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
                Err(e) => return Err(crate::Error::Storage(e.to_string())),
            }
        }

        // Mark tasks done when their issues disappear from JQL results
        let synced_keys = self.repo.list_synced_keys(&mapping.jira_project)?;
        for key in synced_keys {
            if seen_keys.contains(&key) {
                continue;
            }
            match self.task_provider.get(&key) {
                Ok(task) => {
                    if task.status == Status::Done {
                        continue;
                    }
                    // Guard: don't mark done if task has active children
                    let children = self.task_provider.list(ListFilter {
                        parent_key: Some(Some(key.clone())),
                        ..Default::default()
                    })?;
                    if children.iter().any(|c| c.status != Status::Done) {
                        continue;
                    }

                    self.task_provider.update(
                        &key,
                        UpdateParams {
                            status: Some(Status::Done),
                            ..Default::default()
                        },
                    )?;
                    self.repo.mark_departed(&[&key])?;
                    result.done += 1;

                    // Writeback is best-effort — don't abort sync on failure
                    if let Some(wb_config) = &mapping.writeback {
                        if let Err(e) = self
                            .writeback
                            .on_status_change(&key, WritebackAction::Complete, wb_config)
                            .await
                        {
                            warn!(key = %key, error = %e, "writeback failed for auto-done task");
                        }
                    }
                }
                Err(planeai_tasks::provider::Error::NotFound) => {}
                Err(e) => {
                    warn!(key = %key, error = %e, "failed to look up task for disappeared issue");
                }
            }
        }

        Ok(())
    }
}

fn map_status(jira_status: &str, status_map: &std::collections::HashMap<String, String>) -> Status {
    status_map
        .get(jira_status)
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
    use crate::config::{JiraConfig, JiraProjectMapping};
    use rusqlite::Connection;
    use std::collections::HashMap;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn test_map_status_found() {
        let mut m = HashMap::new();
        m.insert("In Progress".to_string(), "in_progress".to_string());
        assert_eq!(map_status("In Progress", &m), Status::InProgress);
    }

    #[test]
    fn test_map_status_not_found_defaults_to_todo() {
        assert_eq!(map_status("Unknown", &HashMap::new()), Status::Todo);
    }

    #[test]
    fn test_map_status_invalid_mapped_value_defaults_to_todo() {
        let mut m = HashMap::new();
        m.insert("X".to_string(), "invalid_status".to_string());
        assert_eq!(map_status("X", &m), Status::Todo);
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
        let mut projects = HashMap::new();
        projects.insert(
            "proj".to_string(),
            JiraProjectMapping {
                jira_project: "PROJ".to_string(),
                jql: "project = PROJ".to_string(),
                status_map: HashMap::from([
                    ("In Progress".to_string(), "in_progress".to_string()),
                    ("Done".to_string(), "done".to_string()),
                ]),
                writeback: None,
            },
        );
        JiraConfig {
            site: "https://test.atlassian.net".to_string(),
            sync_interval_ms: 60_000,
            projects,
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
        assert_eq!(result.done, 0);

        // Verify tasks exist
        use planeai_tasks::model::ListFilter;
        use planeai_tasks::provider::TaskProvider;
        let tasks = task_repo.list(ListFilter::default()).unwrap();
        assert_eq!(tasks.len(), 3);
    }

    #[tokio::test]
    async fn sync_marks_done_when_issue_disappears() {
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

        assert_eq!(result.done, 2);
        // Verify tasks are marked done
        use planeai_tasks::provider::TaskProvider;
        let task2 = task_repo.get("PROJ-2").unwrap();
        let task3 = task_repo.get("PROJ-3").unwrap();
        assert_eq!(task2.status, Status::Done);
        assert_eq!(task3.status, Status::Done);
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
    async fn sync_marks_done_for_all_statuses_when_issue_disappears() {
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

        // Both should be marked done
        assert_eq!(result.done, 2);
        use planeai_tasks::provider::TaskProvider;
        let task1 = task_repo.get("PROJ-1").unwrap();
        let task2 = task_repo.get("PROJ-2").unwrap();
        assert_eq!(task1.status, Status::Done);
        assert_eq!(task2.status, Status::Done);
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
        // Add a second project mapping — it'll use the same mock (returns same data)
        config.projects.insert(
            "other".to_string(),
            JiraProjectMapping {
                jira_project: "OTHER".to_string(),
                jql: "project = OTHER".to_string(),
                status_map: HashMap::new(),
                writeback: None,
            },
        );

        let sync = JiraSync::new(client, jira_repo, task_repo, config);
        // Should not panic even if internal errors happen
        let result = sync.sync_now().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn active_children_prevent_done_marking() {
        let server = MockServer::start().await;

        // First sync: parent issue
        Mock::given(method("GET"))
            .and(path("/search/jql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "issues": [issue_json("PROJ-1", "Parent", "To Do")]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = test_client(&server).await;
        let (jira_repo, task_repo) = setup_db();
        let sync = JiraSync::new(client, jira_repo, task_repo.clone(), test_config());

        sync.sync_now().await.unwrap();

        // Create a child task that is still in_progress
        use planeai_tasks::provider::TaskProvider;
        task_repo
            .create(CreateParams {
                key: Some("PROJ-1-child".to_string()),
                title: "Child task".to_string(),
                status: Some(Status::InProgress),
                parent_key: Some("PROJ-1".to_string()),
                ..Default::default()
            })
            .unwrap();

        // Second sync: parent disappears from JQL
        server.reset().await;
        Mock::given(method("GET"))
            .and(path("/search/jql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "issues": []
            })))
            .mount(&server)
            .await;

        let result = sync.sync_now().await.unwrap();

        // Parent should NOT be marked done because child is active
        assert_eq!(result.done, 0);
        let parent = task_repo.get("PROJ-1").unwrap();
        assert_eq!(parent.status, Status::Todo);
    }

    #[tokio::test]
    async fn done_children_allow_done_marking() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/search/jql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "issues": [issue_json("PROJ-1", "Parent", "To Do")]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = test_client(&server).await;
        let (jira_repo, task_repo) = setup_db();
        let sync = JiraSync::new(client, jira_repo, task_repo.clone(), test_config());

        sync.sync_now().await.unwrap();

        // Create a child task that is done
        use planeai_tasks::provider::TaskProvider;
        task_repo
            .create(CreateParams {
                key: Some("PROJ-1-child".to_string()),
                title: "Child task".to_string(),
                status: Some(Status::Done),
                parent_key: Some("PROJ-1".to_string()),
                ..Default::default()
            })
            .unwrap();

        // Second sync: parent disappears
        server.reset().await;
        Mock::given(method("GET"))
            .and(path("/search/jql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "issues": []
            })))
            .mount(&server)
            .await;

        let result = sync.sync_now().await.unwrap();

        // Parent should be marked done since all children are done
        assert_eq!(result.done, 1);
        let parent = task_repo.get("PROJ-1").unwrap();
        assert_eq!(parent.status, Status::Done);
    }

    #[tokio::test]
    async fn writeback_fires_for_auto_done_tasks() {
        let server = MockServer::start().await;

        // First sync: one issue
        Mock::given(method("GET"))
            .and(path("/search/jql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "issues": [issue_json("PROJ-1", "Task", "To Do")]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = test_client(&server).await;
        let (jira_repo, task_repo) = setup_db();

        let mut config = test_config();
        // Enable writeback with on_complete transition
        config.projects.get_mut("proj").unwrap().writeback = Some(crate::config::WritebackConfig {
            on_start: None,
            on_complete: Some("Done".to_string()),
            comment: true,
        });

        let sync = JiraSync::new(client, jira_repo, task_repo, config);
        sync.sync_now().await.unwrap();

        // Second sync: issue disappears; expect transition + comment calls
        server.reset().await;
        Mock::given(method("GET"))
            .and(path("/search/jql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "issues": []
            })))
            .mount(&server)
            .await;

        // Mock transition endpoint
        Mock::given(method("GET"))
            .and(path("/issue/PROJ-1/transitions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "transitions": [{"id": "31", "name": "Done", "to": {"name": "Done"}}]
            })))
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/issue/PROJ-1/transitions"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/issue/PROJ-1/comment"))
            .respond_with(ResponseTemplate::new(201))
            .expect(1)
            .mount(&server)
            .await;

        let result = sync.sync_now().await.unwrap();
        assert_eq!(result.done, 1);
    }

    #[tokio::test]
    async fn sync_stores_source_name_on_issues() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/search/jql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "issues": [issue_json("PROJ-1", "Task", "To Do")]
            })))
            .mount(&server)
            .await;

        let client = test_client(&server).await;
        let (jira_repo, task_repo) = setup_db();
        let sync = JiraSync::new(client, jira_repo.clone(), task_repo, test_config());

        sync.sync_now().await.unwrap();

        let issue = jira_repo.get_issue("PROJ-1").unwrap().unwrap();
        assert_eq!(issue.source_name, "proj");
    }
}

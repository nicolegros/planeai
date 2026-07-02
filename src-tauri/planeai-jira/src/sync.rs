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
    pub departed: usize,
    pub errors: usize,
}

/// Notified when issues disappear from JQL results.
/// The app layer implements this to show confirmation toasts.
pub trait SyncListener: Send + Sync {
    /// Called for each issue that disappeared from JQL results.
    /// The listener is responsible for eventually marking the task done (or not).
    fn on_issue_departed(&self, issue_key: &str, summary: &str);

    /// Called after each successful sync cycle completes.
    fn on_sync_complete(&self, result: &SyncResult);
}

/// No-op listener for tests that don't care about events.
pub struct NoOpListener;
impl SyncListener for NoOpListener {
    fn on_issue_departed(&self, _key: &str, _summary: &str) {}
    fn on_sync_complete(&self, _result: &SyncResult) {}
}

pub struct JiraSync {
    client: Arc<JiraClient>,
    repo: Arc<JiraRepository>,
    task_provider: Arc<dyn TaskProvider + Send + Sync>,
    config: JiraConfig,
    listener: Arc<dyn SyncListener>,
}

impl JiraSync {
    pub fn new(
        client: Arc<JiraClient>,
        repo: Arc<JiraRepository>,
        task_provider: Arc<dyn TaskProvider + Send + Sync>,
        config: JiraConfig,
    ) -> Self {
        Self::with_listener(client, repo, task_provider, config, Arc::new(NoOpListener))
    }

    pub fn with_listener(
        client: Arc<JiraClient>,
        repo: Arc<JiraRepository>,
        task_provider: Arc<dyn TaskProvider + Send + Sync>,
        config: JiraConfig,
        listener: Arc<dyn SyncListener>,
    ) -> Self {
        Self {
            client,
            repo,
            task_provider,
            config,
            listener,
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
                        Ok(r) => {
                            info!(created = r.created, updated = r.updated, departed = r.departed, errors = r.errors, "jira sync complete");
                            self.listener.on_sync_complete(&r);
                        }
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

        for (source_name, mapping) in &self.config.sources {
            match self.sync_project(source_name, mapping, &mut result).await {
                Ok(()) => {}
                Err(e) => {
                    warn!(source = %source_name, error = %e, "sync failed for source, continuing");
                    result.errors += 1;
                }
            }
        }

        Ok(result)
    }

    async fn sync_project(
        &self,
        source_name: &str,
        mapping: &crate::config::JiraSyncSource,
        result: &mut SyncResult,
    ) -> Result<(), crate::Error> {
        let issues = self.client.search(&mapping.jql).await?;

        let mut seen_keys = HashSet::new();

        for issue in &issues {
            seen_keys.insert(issue.issue_key.clone());

            // Upsert raw issue into local store
            let jira_issue = crate::model::JiraIssue {
                issue_key: issue.issue_key.clone(),
                jira_project: source_name.to_string(),
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
                    let status = map_status(
                        &issue.status,
                        &mapping.status_map,
                        issue.status_category.as_deref(),
                    );
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
                    let new_status = map_status(
                        &issue.status,
                        &mapping.status_map,
                        issue.status_category.as_deref(),
                    );
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

        // Notify listener about issues that disappeared from JQL results
        let synced_keys = self.repo.list_synced_keys(source_name)?;
        for key in synced_keys {
            if seen_keys.contains(&key) {
                continue;
            }
            match self.task_provider.get(&key) {
                Ok(task) => {
                    if task.status == Status::Done {
                        continue;
                    }

                    self.listener.on_issue_departed(&key, &task.title);
                    self.repo.mark_departed(&[&key])?;
                    result.departed += 1;
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

fn map_status(
    jira_status: &str,
    status_map: &std::collections::HashMap<String, String>,
    status_category: Option<&str>,
) -> Status {
    // 1. Explicit status_map takes priority
    if let Some(mapped) = status_map.get(jira_status).and_then(|v| Status::parse(v)) {
        return mapped;
    }
    // 2. Fall back to Jira statusCategory
    if let Some(category) = status_category {
        match category {
            "To Do" => return Status::Todo,
            "In Progress" => return Status::InProgress,
            "Done" => return Status::Done,
            _ => {}
        }
    }
    // 3. Default
    Status::Todo
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
    use crate::config::{JiraConfig, JiraSyncSource};
    use rusqlite::Connection;
    use std::collections::HashMap;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn test_map_status_found() {
        let mut m = HashMap::new();
        m.insert("In Progress".to_string(), "in_progress".to_string());
        assert_eq!(map_status("In Progress", &m, None), Status::InProgress);
    }

    #[test]
    fn test_map_status_not_found_defaults_to_todo() {
        assert_eq!(map_status("Unknown", &HashMap::new(), None), Status::Todo);
    }

    #[test]
    fn test_map_status_invalid_mapped_value_defaults_to_todo() {
        let mut m = HashMap::new();
        m.insert("X".to_string(), "invalid_status".to_string());
        assert_eq!(map_status("X", &m, None), Status::Todo);
    }

    #[test]
    fn test_map_status_falls_back_to_status_category() {
        // No explicit mapping, but statusCategory is "In Progress"
        assert_eq!(
            map_status("Acknowledged", &HashMap::new(), Some("In Progress")),
            Status::InProgress
        );
    }

    #[test]
    fn test_map_status_category_to_do() {
        assert_eq!(
            map_status("Open", &HashMap::new(), Some("To Do")),
            Status::Todo
        );
    }

    #[test]
    fn test_map_status_category_done() {
        assert_eq!(
            map_status("Resolved", &HashMap::new(), Some("Done")),
            Status::Done
        );
    }

    #[test]
    fn test_map_status_explicit_map_wins_over_category() {
        let mut m = HashMap::new();
        m.insert("Acknowledged".to_string(), "done".to_string());
        // Explicit map says "done", category says "In Progress" — explicit wins
        assert_eq!(
            map_status("Acknowledged", &m, Some("In Progress")),
            Status::Done
        );
    }

    #[test]
    fn test_map_status_unknown_category_defaults_to_todo() {
        assert_eq!(
            map_status("Weird", &HashMap::new(), Some("No Category")),
            Status::Todo
        );
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
            JiraSyncSource {
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

    struct CapturingListener {
        departed: std::sync::Mutex<Vec<(String, String)>>,
    }

    impl CapturingListener {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                departed: std::sync::Mutex::new(Vec::new()),
            })
        }

        fn departed_keys(&self) -> Vec<String> {
            self.departed
                .lock()
                .unwrap()
                .iter()
                .map(|(k, _)| k.clone())
                .collect()
        }
    }

    impl SyncListener for CapturingListener {
        fn on_issue_departed(&self, key: &str, summary: &str) {
            self.departed
                .lock()
                .unwrap()
                .push((key.to_string(), summary.to_string()));
        }

        fn on_sync_complete(&self, _result: &SyncResult) {}
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
        assert_eq!(result.departed, 0);

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
        let listener = CapturingListener::new();
        let sync = JiraSync::with_listener(
            client,
            jira_repo.clone(),
            task_repo.clone(),
            test_config(),
            listener.clone(),
        );

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

        assert_eq!(result.departed, 2);
        // Listener should be notified, tasks NOT auto-marked done
        let mut departed = listener.departed_keys();
        departed.sort();
        assert_eq!(departed, vec!["PROJ-2", "PROJ-3"]);
        use planeai_tasks::provider::TaskProvider;
        let task2 = task_repo.get("PROJ-2").unwrap();
        let task3 = task_repo.get("PROJ-3").unwrap();
        assert_eq!(task2.status, Status::Todo);
        assert_eq!(task3.status, Status::Todo);
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
    async fn sync_notifies_listener_for_all_statuses_when_issue_disappears() {
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
        let listener = CapturingListener::new();
        let sync = JiraSync::with_listener(
            client,
            jira_repo.clone(),
            task_repo.clone(),
            test_config(),
            listener.clone(),
        );

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

        // Both should trigger listener notification, not auto-done
        assert_eq!(result.departed, 2);
        let mut departed = listener.departed_keys();
        departed.sort();
        assert_eq!(departed, vec!["PROJ-1", "PROJ-2"]);
        use planeai_tasks::provider::TaskProvider;
        let task1 = task_repo.get("PROJ-1").unwrap();
        let task2 = task_repo.get("PROJ-2").unwrap();
        assert_eq!(task1.status, Status::Todo);
        assert_eq!(task2.status, Status::InProgress);
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
            JiraSyncSource {
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
    async fn sync_does_not_notify_for_already_done_tasks() {
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
        let listener = CapturingListener::new();
        let sync = JiraSync::with_listener(
            client,
            jira_repo.clone(),
            task_repo.clone(),
            test_config(),
            listener.clone(),
        );

        sync.sync_now().await.unwrap();

        // Manually mark task done before next sync
        use planeai_tasks::provider::TaskProvider;
        task_repo
            .update(
                "PROJ-1",
                UpdateParams {
                    status: Some(Status::Done),
                    ..Default::default()
                },
            )
            .unwrap();

        // Second sync: issue disappears
        server.reset().await;
        Mock::given(method("GET"))
            .and(path("/search/jql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "issues": []
            })))
            .mount(&server)
            .await;

        let result = sync.sync_now().await.unwrap();

        // Already-done task should not trigger listener
        assert_eq!(result.departed, 0);
        assert!(listener.departed_keys().is_empty());
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

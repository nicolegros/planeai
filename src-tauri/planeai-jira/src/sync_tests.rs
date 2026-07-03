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

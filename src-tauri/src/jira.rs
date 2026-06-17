use std::sync::Arc;

use planeai_jira::auth::JiraAuth;
use planeai_jira::client::JiraClient;
use planeai_jira::repository::JiraRepository;
use planeai_jira::{JiraSync, JiraWriteback};
use rusqlite::Connection;
use tokio_util::sync::CancellationToken;

use crate::config::Config;
use crate::paths;

pub struct JiraState {
    pub sync: Option<Arc<JiraSync>>,
    pub writeback: Option<Arc<JiraWriteback>>,
    pub auth: Arc<JiraAuth>,
    pub repo: Arc<JiraRepository>,
    pub cancel: Option<CancellationToken>,
}

pub fn init_jira(config: &Config) -> Option<JiraState> {
    let jira_config = config.integrations.as_ref()?.jira.as_ref()?;
    let auth = Arc::new(JiraAuth::new(&jira_config.site));

    let db_path = paths::db_path();
    let conn = Connection::open(&db_path).ok()?;
    let repo = Arc::new(JiraRepository::new(conn).ok()?);

    if auth.is_connected() {
        let cloud_id = auth.cloud_id().ok()?;
        let client = Arc::new(JiraClient::new(auth.clone(), cloud_id));

        let task_db_path = db_path.to_str()?;
        // JiraSync needs a TaskProvider — we use a shared prefix "JIRA" for jira-sourced tasks
        let task_provider =
            Arc::new(planeai_tasks::sqlite::SqliteRepository::open(task_db_path, "JIRA").ok()?);

        let sync = Arc::new(JiraSync::new(
            client.clone(),
            repo.clone(),
            task_provider,
            jira_config.clone(),
        ));
        let writeback = Arc::new(JiraWriteback::new(client));
        let cancel = CancellationToken::new();

        Some(JiraState {
            sync: Some(sync),
            writeback: Some(writeback),
            auth,
            repo,
            cancel: Some(cancel),
        })
    } else {
        Some(JiraState {
            sync: None,
            writeback: None,
            auth,
            repo,
            cancel: None,
        })
    }
}

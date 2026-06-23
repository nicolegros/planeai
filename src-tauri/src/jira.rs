use std::sync::Arc;

use planeai_jira::auth::JiraAuth;
use planeai_jira::client::JiraClient;
use planeai_jira::config::JiraConfig;
use planeai_jira::repository::JiraRepository;
use planeai_jira::{JiraSync, JiraWriteback, WritebackAction};
use planeai_tasks::model::Status;
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

impl JiraState {
    /// Activate sync + writeback. Returns the CancellationToken for the sync loop.
    pub fn activate(&mut self, jira_config: &JiraConfig) -> Result<CancellationToken, String> {
        let cloud_id = self.auth.cloud_id().map_err(|e| e.to_string())?;
        let client = Arc::new(JiraClient::new(self.auth.clone(), cloud_id));
        let task_provider = open_task_provider(jira_config)?;
        let cancel = CancellationToken::new();

        self.sync = Some(Arc::new(JiraSync::new(
            client.clone(),
            self.repo.clone(),
            task_provider,
            jira_config.clone(),
        )));
        self.writeback = Some(Arc::new(JiraWriteback::new(client)));
        self.cancel = Some(cancel.clone());
        Ok(cancel)
    }

    /// Deactivate: cancel sync loop and clear client state.
    pub fn deactivate(&mut self) {
        if let Some(c) = &self.cancel {
            c.cancel();
        }
        self.sync = None;
        self.writeback = None;
        self.cancel = None;
    }

    /// Trigger async writeback if this task is Jira-sourced. Non-blocking.
    pub fn try_writeback(&self, task_key: &str, status: Status, config: &Config) {
        let action = match status {
            Status::InProgress => WritebackAction::Start,
            Status::Done => WritebackAction::Complete,
            _ => return,
        };
        let writeback = match &self.writeback {
            Some(wb) => wb.clone(),
            None => return,
        };
        let issue_key = match self.repo.get_task_issue_key(task_key) {
            Ok(Some(k)) => k,
            _ => return,
        };
        let wb_config = (|| {
            let jira_cfg = config.integrations.as_ref()?.jira.as_ref()?;
            let issue_proj = self.repo.get_issue(&issue_key).ok()??.jira_project;
            jira_cfg
                .projects
                .values()
                .find(|m| m.jira_project == issue_proj)?
                .writeback
                .clone()
        })();
        if let Some(wb_config) = wb_config {
            tokio::spawn(async move {
                if let Err(e) = writeback
                    .on_status_change(&issue_key, action, &wb_config)
                    .await
                {
                    tracing::warn!(error = %e, "jira writeback failed");
                }
            });
        }
    }
}

pub fn init_jira(config: &Config) -> Option<JiraState> {
    let jira_config = config.integrations.as_ref()?.jira.as_ref()?;
    let token_dir = paths::app_data_dir().join("jira-tokens");
    let auth = Arc::new(JiraAuth::new(&jira_config.site, token_dir));

    let db_path = paths::db_path();
    let conn = match Connection::open(&db_path) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error = %e, "jira: failed to open database");
            return None;
        }
    };
    let repo = match JiraRepository::new(conn) {
        Ok(r) => Arc::new(r),
        Err(e) => {
            tracing::warn!(error = %e, "jira: failed to initialize repository");
            return None;
        }
    };

    let mut state = JiraState {
        sync: None,
        writeback: None,
        auth,
        repo,
        cancel: None,
    };

    if state.auth.is_connected() {
        if let Err(e) = state.activate(jira_config) {
            tracing::warn!(error = %e, "jira: configured but failed to activate");
        }
    }

    Some(state)
}

fn open_task_provider(
    config: &JiraConfig,
) -> Result<Arc<dyn planeai_tasks::provider::TaskProvider + Send + Sync>, String> {
    let db_path = paths::db_path();
    let path_str = db_path.to_str().ok_or("invalid db path")?;
    // Use first project key as prefix; falls back to "JIRA" if no projects configured
    let prefix = config
        .projects
        .keys()
        .next()
        .map(|k| planeai_tasks::sqlite::derive_prefix(k))
        .unwrap_or_else(|| "JIRA".to_string());
    planeai_tasks::sqlite::SqliteRepository::open(path_str, &prefix)
        .map(|r| Arc::new(r) as Arc<dyn planeai_tasks::provider::TaskProvider + Send + Sync>)
        .map_err(|e| e.to_string())
}

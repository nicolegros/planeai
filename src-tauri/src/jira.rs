use std::sync::Arc;

use planeai_jira::auth::JiraAuth;
use planeai_jira::client::JiraClient;
use planeai_jira::config::JiraConfig;
use planeai_jira::repository::JiraRepository;
use planeai_jira::{JiraSync, JiraWriteback, SyncListener, WritebackAction};
use planeai_tasks::model::Status;
use rusqlite::Connection;
use tauri::{AppHandle, Emitter};
use tokio_util::sync::CancellationToken;

use crate::config::Config;

/// Emits Tauri events when issues disappear from JQL results.
pub struct TauriSyncListener {
    app: AppHandle,
}

impl TauriSyncListener {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
struct JiraIssueDeparted {
    key: String,
    summary: String,
}

impl SyncListener for TauriSyncListener {
    fn on_issue_departed(&self, key: &str, summary: &str) {
        let payload = JiraIssueDeparted {
            key: key.to_string(),
            summary: summary.to_string(),
        };
        if let Err(e) = self.app.emit("jira-issue-departed", &payload) {
            tracing::warn!(error = %e, key = %key, "failed to emit jira-issue-departed event");
        }
    }

    fn on_sync_complete(&self, _result: &planeai_jira::SyncResult) {
        let _ = self.app.emit("jira-sync-complete", ());
    }
}

pub struct JiraState {
    pub sync: Option<Arc<JiraSync>>,
    pub writeback: Option<Arc<JiraWriteback>>,
    pub auth: Arc<JiraAuth>,
    pub repo: Arc<JiraRepository>,
    pub cancel: Option<CancellationToken>,
}

impl JiraState {
    /// Activate sync + writeback. Returns the CancellationToken for the sync loop.
    pub fn activate(
        &mut self,
        jira_config: &JiraConfig,
        app: AppHandle,
    ) -> Result<CancellationToken, String> {
        let cloud_id = self.auth.cloud_id().map_err(|e| e.to_string())?;
        let client = Arc::new(JiraClient::new(self.auth.clone(), cloud_id));
        let task_provider = open_task_provider(jira_config)?;
        let cancel = CancellationToken::new();
        let listener = Arc::new(TauriSyncListener::new(app));

        self.sync = Some(Arc::new(JiraSync::with_listener(
            client.clone(),
            self.repo.clone(),
            task_provider,
            jira_config.clone(),
            listener,
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
    /// After PLA-148, the task key IS the Jira issue key for synced tasks.
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
        // Check if this task_key corresponds to a known Jira issue
        let issue = match self.repo.get_issue(task_key) {
            Ok(Some(i)) => i,
            _ => return,
        };
        let wb_config = (|| {
            let jira_cfg = config.integrations.as_ref()?.jira.as_ref()?;
            // Look up writeback config by source_name
            if !issue.source_name.is_empty() {
                if let Some(source) = jira_cfg.sources.get(&issue.source_name) {
                    return source.writeback.clone();
                }
            }
            None
        })();
        if let Some(wb_config) = wb_config {
            let issue_key = task_key.to_string();
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

pub fn init_jira(config: &Config, app: AppHandle) -> Option<JiraState> {
    let jira_config = config.integrations.as_ref()?.jira.as_ref()?;
    let token_dir = planeai_paths::app_data_dir().join("jira-tokens");
    let auth = Arc::new(JiraAuth::new(&jira_config.site, token_dir));

    let db_path = planeai_paths::db_path();
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
        if let Err(e) = state.activate(jira_config, app) {
            tracing::warn!(error = %e, "jira: configured but failed to activate");
        }
    }

    Some(state)
}

pub fn open_task_provider(
    config: &JiraConfig,
) -> Result<Arc<dyn planeai_tasks::provider::TaskProvider + Send + Sync>, String> {
    let db_path = planeai_paths::db_path();
    let path_str = db_path.to_str().ok_or("invalid db path")?;
    // Derive prefix from the Jira site hostname for determinism.
    // After PLA-148 all Jira tasks use explicit keys, so this only affects the
    // task_projects registration (auto-generated keys are unused for Jira sync).
    let prefix = config
        .site
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('.')
        .next()
        .map(planeai_tasks::sqlite::derive_prefix)
        .unwrap_or_else(|| "JIRA".to_string());
    planeai_tasks::sqlite::SqliteRepository::open(path_str, &prefix)
        .map(|r| Arc::new(r) as Arc<dyn planeai_tasks::provider::TaskProvider + Send + Sync>)
        .map_err(|e| e.to_string())
}

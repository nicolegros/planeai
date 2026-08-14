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
    /// Exact settings used to construct the active sync/writeback runtime.
    pub sync_config: Option<JiraConfig>,
    /// An activation is being prepared off the IPC thread.
    pub activating: bool,
}

/// Cloneable state required to build sync clients and repositories off the IPC thread.
pub(crate) struct JiraActivationInputs {
    auth: Arc<JiraAuth>,
    repo: Arc<JiraRepository>,
}

/// Fully-built sync state awaiting a brief, lock-only installation into `JiraState`.
pub(crate) struct PreparedJiraActivation {
    sync: Arc<JiraSync>,
    writeback: Arc<JiraWriteback>,
    cancel: CancellationToken,
    config: JiraConfig,
}

impl JiraState {
    pub(crate) fn activation_inputs(&self) -> JiraActivationInputs {
        JiraActivationInputs {
            auth: self.auth.clone(),
            repo: self.repo.clone(),
        }
    }

    /// Perform all filesystem and SQLite work required to activate Jira.
    /// Call this through `commands::blocking`, never while holding `JiraHandle`.
    pub(crate) fn prepare_activation(
        inputs: JiraActivationInputs,
        jira_config: JiraConfig,
        app: AppHandle,
    ) -> Result<PreparedJiraActivation, String> {
        let cloud_id = inputs.auth.cloud_id().map_err(|e| e.to_string())?;
        let client = Arc::new(JiraClient::new(inputs.auth, cloud_id));
        let task_provider = open_task_provider(&jira_config)?;
        let cancel = CancellationToken::new();
        let listener = Arc::new(TauriSyncListener::new(app));
        let sync_config = jira_config.clone();
        let sync = Arc::new(JiraSync::with_listener(
            client.clone(),
            inputs.repo,
            task_provider,
            jira_config,
            listener,
        ));
        let writeback = Arc::new(JiraWriteback::new(client));

        Ok(PreparedJiraActivation {
            sync,
            writeback,
            cancel,
            config: sync_config,
        })
    }

    /// True only when the active runtime was built from exactly these Jira settings.
    pub(crate) fn sync_matches_config(&self, jira_config: &JiraConfig) -> bool {
        self.sync_config.as_ref() == Some(jira_config)
    }

    /// Install already-prepared state. This intentionally performs no I/O.
    pub(crate) fn install_activation(
        &mut self,
        prepared: PreparedJiraActivation,
    ) -> Result<CancellationToken, String> {
        if self.sync.is_some() || self.writeback.is_some() || self.cancel.is_some() {
            return Err("jira sync is already initialized".to_string());
        }
        if !self.auth.is_connection_active() {
            return Err(
                "Jira authorization was disconnected before activation completed".to_string(),
            );
        }

        self.auth.set_sync_cancellation(prepared.cancel.clone());
        self.sync = Some(prepared.sync);
        self.writeback = Some(prepared.writeback);
        self.sync_config = Some(prepared.config);
        self.cancel = Some(prepared.cancel.clone());
        Ok(prepared.cancel)
    }

    /// Deactivate: cancel sync loop and clear client state.
    pub fn deactivate(&mut self) {
        if let Some(c) = &self.cancel {
            c.cancel();
        }
        self.sync = None;
        self.writeback = None;
        self.sync_config = None;
        self.cancel = None;
        self.activating = false;
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

/// Construct Jira state, including token-store probing and Jira repository migration.
/// Commands must call this through `commands::blocking`.
pub(crate) fn construct_jira_state(
    jira_config: JiraConfig,
    app: AppHandle,
) -> Result<JiraState, String> {
    let token_dir = planeai_paths::app_data_dir().join("jira-tokens");
    let auth = Arc::new(JiraAuth::new(&jira_config.site, token_dir));
    let connection_state_app = app.clone();
    auth.set_connection_state_listener(Arc::new(move || {
        if let Err(error) = connection_state_app.emit("jira-connection-state-changed", ()) {
            tracing::warn!(error = %error, "failed to emit Jira connection state change");
        }
    }));

    let db_path = planeai_paths::db_path();
    let conn = Connection::open(&db_path).map_err(|error| {
        tracing::warn!(error = %error, "jira: failed to open database");
        error.to_string()
    })?;
    let repo = Arc::new(JiraRepository::new(conn).map_err(|error| {
        tracing::warn!(error = %error, "jira: failed to initialize repository");
        error.to_string()
    })?);

    Ok(JiraState {
        sync: None,
        writeback: None,
        auth,
        repo,
        cancel: None,
        sync_config: None,
        activating: false,
    })
}

/// Startup-only Jira initialization. Tauri commands use `construct_jira_state` and
/// `prepare_activation` through `commands::blocking` instead.
pub fn init_jira(config: &Config, app: AppHandle) -> Option<JiraState> {
    let jira_config = config.integrations.as_ref()?.jira.as_ref()?.clone();
    let mut state = construct_jira_state(jira_config.clone(), app.clone()).ok()?;

    if state.auth.is_connected() {
        match JiraState::prepare_activation(state.activation_inputs(), jira_config, app) {
            Ok(prepared) => {
                if let Err(error) = state.install_activation(prepared) {
                    tracing::warn!(error = %error, "jira: configured but failed to activate");
                }
            }
            Err(error) => tracing::warn!(error = %error, "jira: configured but failed to activate"),
        }
    }

    Some(state)
}

/// Open the shared task provider for all Jira-synced issues.
///
/// All sources within a single Jira site share one prefix namespace. This is safe because:
/// - Jira issue keys are globally unique within a site (e.g., PROJ-1, ENG-42)
/// - Synced tasks use the Jira issue key directly (not auto-generated sequential keys)
/// - The prefix only affects the `task_projects` registration; it's never used for key generation
///   in the Jira sync path
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[tokio::test]
    async fn installed_manual_sync_reuses_auth_cancellation_and_disconnects_immediately() {
        let token_dir = tempfile::tempdir().unwrap();
        std::fs::write(token_dir.path().join("refresh_token"), "refresh").unwrap();
        std::fs::write(token_dir.path().join("cloud_id"), "cloud").unwrap();
        std::fs::write(token_dir.path().join("connection_cleared"), "false").unwrap();
        let auth = Arc::new(JiraAuth::new(
            "https://test.atlassian.net",
            token_dir.path().to_path_buf(),
        ));
        let repo = Arc::new(JiraRepository::new(Connection::open_in_memory().unwrap()).unwrap());
        let task_provider =
            Arc::new(planeai_tasks::sqlite::SqliteRepository::open_in_memory("TST").unwrap())
                as Arc<dyn planeai_tasks::provider::TaskProvider + Send + Sync>;
        let config = JiraConfig {
            site: "https://test.atlassian.net".to_string(),
            sync_interval_ms: 60_000,
            sources: HashMap::new(),
        };
        let client = Arc::new(JiraClient::new(auth.clone(), "cloud".to_string()));
        let sync = Arc::new(JiraSync::new(
            client.clone(),
            repo.clone(),
            task_provider,
            config.clone(),
        ));
        let cancel = CancellationToken::new();
        let mut state = JiraState {
            sync: None,
            writeback: None,
            auth: auth.clone(),
            repo,
            cancel: None,
            sync_config: None,
            activating: false,
        };

        // `install_activation` only attaches pre-built objects; it does not perform I/O.
        let installed_cancel = state
            .install_activation(PreparedJiraActivation {
                sync: sync.clone(),
                writeback: Arc::new(JiraWriteback::new(client)),
                cancel,
                config: config.clone(),
            })
            .unwrap();

        assert!(Arc::ptr_eq(state.sync.as_ref().unwrap(), &sync));
        let handle = crate::commands::jira::JiraHandle::new(None);
        let mut slot = handle.0.lock().await;
        slot.state = Some(state);

        let registration_handle = handle.clone();
        let registration = tokio::spawn(async move {
            registration_handle
                .install_runtime_deactivation_listener()
                .await;
        });
        tokio::task::yield_now().await;
        assert!(
            !registration.is_finished(),
            "listener registration must wait for the Jira slot rather than silently skipping"
        );
        drop(slot);
        registration.await.unwrap();

        auth.disconnect().await.unwrap();
        assert!(installed_cancel.is_cancelled());

        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                let slot = handle.0.lock().await;
                if slot.state.as_ref().is_some_and(|state| {
                    state.sync.is_none() && state.writeback.is_none() && state.cancel.is_none()
                }) {
                    break;
                }
                drop(slot);
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("connection-state event should deactivate stale Jira runtime state");
    }
}

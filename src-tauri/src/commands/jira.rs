use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::State;
use tokio_util::sync::CancellationToken;

use planeai_jira::client::JiraClient;
use planeai_jira::{JiraSync, JiraWriteback};

use crate::jira::JiraState;
use crate::paths;
use crate::state::ConfigState;

pub struct JiraHandle(pub Mutex<Option<JiraState>>);

#[derive(Serialize)]
pub struct JiraStatusResponse {
    pub connected: bool,
    pub site: Option<String>,
}

#[derive(Serialize)]
pub struct SyncResultResponse {
    pub created: usize,
    pub updated: usize,
    pub stale: usize,
}

#[tauri::command]
pub async fn jira_connect(
    jira: State<'_, JiraHandle>,
    config_state: State<'_, ConfigState>,
) -> Result<(), String> {
    let (auth, repo) = {
        let guard = jira.0.lock().map_err(|e| e.to_string())?;
        let state = guard.as_ref().ok_or("jira not configured")?;
        (state.auth.clone(), state.repo.clone())
    };

    auth.connect().await.map_err(|e| e.to_string())?;

    let jira_config = {
        let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
        cfg.integrations
            .as_ref()
            .and_then(|i| i.jira.clone())
            .ok_or("jira config missing")?
    };

    let cloud_id = auth.cloud_id().map_err(|e| e.to_string())?;
    let client = Arc::new(JiraClient::new(auth.clone(), cloud_id));
    let task_provider = open_task_provider()?;
    let sync = Arc::new(JiraSync::new(
        client.clone(),
        repo.clone(),
        task_provider,
        jira_config,
    ));
    let writeback = Arc::new(JiraWriteback::new(client));
    let cancel = CancellationToken::new();

    // Start sync loop
    let sync_clone = sync.clone();
    let cancel_clone = cancel.clone();
    tokio::spawn(async move { sync_clone.start(cancel_clone).await });

    {
        let mut guard = jira.0.lock().map_err(|e| e.to_string())?;
        if let Some(state) = guard.as_mut() {
            state.sync = Some(sync);
            state.writeback = Some(writeback);
            state.cancel = Some(cancel);
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn jira_disconnect(jira: State<'_, JiraHandle>) -> Result<(), String> {
    let (auth, _cancel) = {
        let guard = jira.0.lock().map_err(|e| e.to_string())?;
        let state = guard.as_ref().ok_or("jira not configured")?;
        if let Some(cancel) = &state.cancel {
            cancel.cancel();
        }
        (state.auth.clone(), state.cancel.clone())
    };

    auth.disconnect().await.map_err(|e| e.to_string())?;

    {
        let mut guard = jira.0.lock().map_err(|e| e.to_string())?;
        if let Some(state) = guard.as_mut() {
            state.sync = None;
            state.writeback = None;
            state.cancel = None;
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn jira_sync_now(jira: State<'_, JiraHandle>) -> Result<SyncResultResponse, String> {
    let sync = {
        let guard = jira.0.lock().map_err(|e| e.to_string())?;
        let state = guard.as_ref().ok_or("jira not configured")?;
        state.sync.clone().ok_or("jira not connected")?
    };

    let result = sync.sync_now().await.map_err(|e| e.to_string())?;
    Ok(SyncResultResponse {
        created: result.created,
        updated: result.updated,
        stale: result.stale,
    })
}

#[tauri::command]
pub fn jira_status(
    jira: State<'_, JiraHandle>,
    config_state: State<'_, ConfigState>,
) -> JiraStatusResponse {
    let connected = jira
        .0
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|s| s.auth.is_connected()))
        .unwrap_or(false);

    let site = config_state
        .0
        .lock()
        .ok()
        .and_then(|cfg| cfg.integrations.as_ref()?.jira.as_ref().map(|j| j.site.clone()));

    JiraStatusResponse { connected, site }
}

fn open_task_provider() -> Result<Arc<planeai_tasks::sqlite::SqliteRepository>, String> {
    let db_path = paths::db_path();
    let path_str = db_path.to_str().ok_or("invalid db path")?;
    planeai_tasks::sqlite::SqliteRepository::open(path_str, "JIRA").map_err(|e| e.to_string()).map(Arc::new)
}

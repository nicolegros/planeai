use serde::Serialize;
use tauri::State;
use tokio::sync::Mutex;

use planeai_jira::SyncResult;

use crate::jira::JiraState;
use crate::state::ConfigState;

pub struct JiraHandle(pub Mutex<Option<JiraState>>);

#[derive(Serialize)]
pub struct JiraStatusResponse {
    pub connected: bool,
    pub site: Option<String>,
}

#[tauri::command]
pub async fn jira_connect(
    jira: State<'_, JiraHandle>,
    config_state: State<'_, ConfigState>,
) -> Result<(), String> {
    let mut guard = jira.0.lock().await;

    let jira_config = {
        let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
        *guard = crate::jira::init_jira(&cfg);
        cfg.integrations
            .as_ref()
            .and_then(|i| i.jira.clone())
            .ok_or("jira not configured — add a site URL first")?
    };

    let state = guard.as_mut().ok_or("jira not configured")?;
    state.auth.connect().await.map_err(|e| e.to_string())?;

    let cancel = state.activate(&jira_config)?;
    let sync = state.sync.clone().unwrap();
    tokio::spawn(async move { sync.start(cancel).await });

    tracing::info!("jira: connected");
    Ok(())
}

#[tauri::command]
pub async fn jira_disconnect(jira: State<'_, JiraHandle>) -> Result<(), String> {
    let mut guard = jira.0.lock().await;
    let state = guard.as_mut().ok_or("jira not configured")?;
    state.deactivate();
    state.auth.disconnect().await.map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn jira_sync_now(
    jira: State<'_, JiraHandle>,
    config_state: State<'_, ConfigState>,
) -> Result<SyncResult, String> {
    let mut guard = jira.0.lock().await;
    let state = guard.as_mut().ok_or("jira not configured")?;

    let jira_config = {
        let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
        cfg.integrations
            .as_ref()
            .and_then(|i| i.jira.clone())
            .ok_or("jira config missing")?
    };

    let _ = state.activate(&jira_config);
    let sync = state.sync.clone().ok_or("jira not connected")?;
    drop(guard);

    sync.sync_now().await.map_err(|e| e.to_string())
}

/// Read-only status check. Does not activate sync or mutate state.
#[tauri::command]
pub async fn jira_status(
    jira: State<'_, JiraHandle>,
    config_state: State<'_, ConfigState>,
) -> Result<JiraStatusResponse, String> {
    let guard = jira.0.lock().await;

    let connected = guard
        .as_ref()
        .map(|s| s.auth.is_connected())
        .unwrap_or(false);

    let site = config_state.0.lock().ok().and_then(|cfg| {
        cfg.integrations
            .as_ref()?
            .jira
            .as_ref()
            .map(|j| j.site.clone())
    });

    Ok(JiraStatusResponse { connected, site })
}

use std::sync::Mutex;

use serde::Serialize;
use tauri::State;

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
    {
        let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
        let mut guard = jira.0.lock().map_err(|e| e.to_string())?;
        *guard = crate::jira::init_jira(&cfg);
    }

    let auth = {
        let guard = jira.0.lock().map_err(|e| e.to_string())?;
        let state = guard
            .as_ref()
            .ok_or("jira not configured — add a site URL first")?;
        state.auth.clone()
    };

    auth.connect().await.map_err(|e| e.to_string())?;

    let jira_config = {
        let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
        cfg.integrations
            .as_ref()
            .and_then(|i| i.jira.clone())
            .ok_or("jira config missing")?
    };

    let cancel = {
        let mut guard = jira.0.lock().map_err(|e| e.to_string())?;
        let state = guard.as_mut().ok_or("jira not configured")?;
        state.activate(&jira_config)?
    };

    let sync = {
        let guard = jira.0.lock().map_err(|e| e.to_string())?;
        guard.as_ref().unwrap().sync.clone().unwrap()
    };
    tokio::spawn(async move { sync.start(cancel).await });

    tracing::info!("jira: connected");
    Ok(())
}

#[tauri::command]
pub async fn jira_disconnect(jira: State<'_, JiraHandle>) -> Result<(), String> {
    let auth = {
        let mut guard = jira.0.lock().map_err(|e| e.to_string())?;
        let state = guard.as_mut().ok_or("jira not configured")?;
        state.deactivate();
        state.auth.clone()
    };

    auth.disconnect().await.map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn jira_sync_now(
    jira: State<'_, JiraHandle>,
    config_state: State<'_, ConfigState>,
) -> Result<SyncResult, String> {
    let jira_config = {
        let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
        cfg.integrations
            .as_ref()
            .and_then(|i| i.jira.clone())
            .ok_or("jira config missing")?
    };

    {
        let mut guard = jira.0.lock().map_err(|e| e.to_string())?;
        let state = guard.as_mut().ok_or("jira not configured")?;
        let _ = state.activate(&jira_config);
    }

    let sync = {
        let guard = jira.0.lock().map_err(|e| e.to_string())?;
        let state = guard.as_ref().ok_or("jira not configured")?;
        state.sync.clone().ok_or("jira not connected")?
    };

    sync.sync_now().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn jira_status(
    jira: State<'_, JiraHandle>,
    config_state: State<'_, ConfigState>,
) -> JiraStatusResponse {
    {
        let mut guard = jira.0.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_none() {
            if let Ok(cfg) = config_state.0.lock() {
                *guard = crate::jira::init_jira(&cfg);
            }
        }
        let should_activate = guard
            .as_ref()
            .map(|s| s.auth.is_connected() && s.cancel.is_none())
            .unwrap_or(false);
        if should_activate {
            let jira_cfg = config_state
                .0
                .lock()
                .ok()
                .and_then(|cfg| cfg.integrations.as_ref()?.jira.clone());
            if let Some(jira_cfg) = jira_cfg {
                let state = guard.as_mut().unwrap();
                if let Ok(cancel) = state.activate(&jira_cfg) {
                    let sync = state.sync.clone().unwrap();
                    tokio::spawn(async move { sync.start(cancel).await });
                }
            }
        }
    }

    let connected = jira
        .0
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|s| s.auth.is_connected()))
        .unwrap_or(false);

    let site = config_state.0.lock().ok().and_then(|cfg| {
        cfg.integrations
            .as_ref()?
            .jira
            .as_ref()
            .map(|j| j.site.clone())
    });

    JiraStatusResponse { connected, site }
}

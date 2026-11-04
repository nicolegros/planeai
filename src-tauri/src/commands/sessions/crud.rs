use tauri::{AppHandle, State};

use crate::commands::sessions::lifecycle::{exit_active_session, session_transition};
use crate::db;
use crate::plugins::PluginRuntimeHandle;
use crate::state::{ConfigState, DbState, McpServerState, NotifyHandle};

use super::helpers::register_notify_session;

#[tauri::command]
pub fn create_session(
    state: State<DbState>,
    notify: State<NotifyHandle>,
    config_state: State<ConfigState>,
    project_id: String,
    name: String,
    tmux_name: String,
    branch: String,
) -> Result<db::Session, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let session = db::create_session(&conn, &project_id, &name, &tmux_name, &branch, None)
        .map_err(|e| e.to_string())?;

    // Register in NotifyState at creation time
    let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
    let project_name = db::get_project(&conn, &project_id)
        .ok()
        .flatten()
        .map(|p| p.name)
        .unwrap_or_else(|| "unknown".to_string());
    let mut ns = notify.0.lock().unwrap();
    register_notify_session(&mut ns, &session, &project_name, &cfg);

    Ok(session)
}

#[tauri::command]
pub fn list_sessions(state: State<DbState>) -> Result<Vec<db::Session>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    db::list_sessions(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn rename_session(
    state: State<DbState>,
    notify: State<NotifyHandle>,
    config_state: State<ConfigState>,
    id: String,
    name: String,
) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    db::rename_session(&conn, &id, &name).map_err(|e| e.to_string())?;
    let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
    if let Some(session) = db::get_session(&conn, &id).map_err(|e| e.to_string())? {
        let project_name = db::get_project(&conn, &session.project_id)
            .ok()
            .flatten()
            .map(|p| p.name)
            .unwrap_or_else(|| "unknown".to_string());
        let mut ns = notify.0.lock().unwrap();
        register_notify_session(&mut ns, &session, &project_name, &cfg);
    }
    Ok(())
}

#[tauri::command]
pub fn list_archived_sessions(state: State<DbState>) -> Result<Vec<db::Session>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    db::list_archived_sessions(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn restore_session(
    state: State<'_, DbState>,
    notify: State<'_, NotifyHandle>,
    config_state: State<'_, ConfigState>,
    runtime: State<'_, PluginRuntimeHandle>,
    mcp: State<'_, McpServerState>,
    id: String,
) -> Result<(), String> {
    let cfg = config_state.0.lock().map_err(|e| e.to_string())?.clone();
    let (previous_status, session) =
        super::lifecycle::restart_blocking(state.0.clone(), cfg.clone(), mcp.0.url(), id).await?;
    runtime
        .0
        .dispatch_session_lifecycle(session_transition(&session, &previous_status, "active"));

    let project_name = {
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        db::get_project(&conn, &session.project_id)
            .ok()
            .flatten()
            .map(|p| p.name)
            .unwrap_or_else(|| "unknown".to_string())
    };
    let mut ns = notify.0.lock().unwrap();
    register_notify_session(&mut ns, &session, &project_name, &cfg);

    Ok(())
}

#[tauri::command]
pub fn delete_session(state: State<DbState>, id: String) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    db::delete_session(&conn, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn acknowledge_session(session_id: String, notify: State<NotifyHandle>) {
    let mut ns = notify.0.lock().unwrap();
    ns.acknowledge(&session_id);
}

#[tauri::command]
pub async fn mark_exited(
    session_id: String,
    app: AppHandle,
    db_state: State<'_, DbState>,
    runtime: State<'_, PluginRuntimeHandle>,
) -> Result<(), String> {
    let db = db_state.0.clone();
    let id = session_id.clone();
    let exited = crate::commands::blocking(move || {
        let conn = db.lock().map_err(|e| e.to_string())?;
        exit_active_session(&conn, &id)
    })
    .await?;
    if let Some(transition) = exited {
        crate::task_activity::record(&app, &session_id, crate::task_activity::Activity::Exited);
        runtime.0.dispatch_session_lifecycle(transition);
    }
    Ok(())
}

#[tauri::command]
pub fn save_mru_order(session_ids: Vec<String>, db_state: State<DbState>) -> Result<(), String> {
    let conn = db_state.0.lock().map_err(|e| e.to_string())?;
    let refs: Vec<&str> = session_ids.iter().map(|s| s.as_str()).collect();
    db::save_mru_order(&conn, &refs).map_err(|e| e.to_string())
}

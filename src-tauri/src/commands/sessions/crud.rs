use tauri::State;

use crate::db;
use crate::state::{ConfigState, DbState, NotifyHandle};

use super::helpers::provider_has_hook;

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
    let display_name = if name.is_empty() { &branch } else { &name };
    let hook_enabled = session
        .provider
        .as_deref()
        .map(|pk| provider_has_hook(pk, &cfg))
        .unwrap_or(false);
    let mut ns = notify.0.lock().unwrap();
    ns.register_session(&session.id, display_name, &project_name, hook_enabled);

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
        let display_name = if name.is_empty() {
            &session.branch
        } else {
            &name
        };
        let hook_enabled = session
            .provider
            .as_deref()
            .map(|pk| provider_has_hook(pk, &cfg))
            .unwrap_or(false);
        let mut ns = notify.0.lock().unwrap();
        ns.register_session(&id, display_name, &project_name, hook_enabled);
    }
    Ok(())
}

#[tauri::command]
pub fn list_archived_sessions(state: State<DbState>) -> Result<Vec<db::Session>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    db::list_archived_sessions(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn restore_session(
    state: State<DbState>,
    notify: State<NotifyHandle>,
    config_state: State<ConfigState>,
    id: String,
) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    db::restore_session(&conn, &id).map_err(|e| e.to_string())?;

    // Register in NotifyState when restoring
    if let Some(session) = db::get_session(&conn, &id).map_err(|e| e.to_string())? {
        let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
        let project_name = db::get_project(&conn, &session.project_id)
            .ok()
            .flatten()
            .map(|p| p.name)
            .unwrap_or_else(|| "unknown".to_string());
        let display_name = if session.name.is_empty() {
            &session.branch
        } else {
            &session.name
        };
        let hook_enabled = session
            .provider
            .as_deref()
            .map(|pk| provider_has_hook(pk, &cfg))
            .unwrap_or(false);
        let mut ns = notify.0.lock().unwrap();
        ns.register_session(&id, display_name, &project_name, hook_enabled);
    }
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
pub fn mark_exited(session_id: String, db_state: State<DbState>) -> Result<(), String> {
    let conn = db_state.0.lock().map_err(|e| e.to_string())?;
    db::mark_session_exited(&conn, &session_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_mru_order(session_ids: Vec<String>, db_state: State<DbState>) -> Result<(), String> {
    let conn = db_state.0.lock().map_err(|e| e.to_string())?;
    let refs: Vec<&str> = session_ids.iter().map(|s| s.as_str()).collect();
    db::save_mru_order(&conn, &refs).map_err(|e| e.to_string())
}

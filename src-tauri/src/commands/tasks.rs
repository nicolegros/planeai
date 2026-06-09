use tauri::State;

use crate::db;
use crate::state::{ConfigState, DbState};
use crate::task_manager;

use crate::commands::pr::poll_pr_for_session;
use crate::commands::sessions::helpers::{fire_task_hook, resolve_task_manager, session_cwd};

#[tauri::command]
pub fn get_task_details(
    config_state: State<ConfigState>,
    key: String,
    repo_path: String,
) -> Result<task_manager::TaskItem, String> {
    let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
    let tm = resolve_task_manager(&cfg)?;
    task_manager::get_task(tm, &key, std::path::Path::new(&repo_path))
}

#[tauri::command]
pub fn list_task_items(
    config_state: State<ConfigState>,
    repo_path: String,
) -> Result<Vec<task_manager::TaskItem>, String> {
    let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
    let tm = resolve_task_manager(&cfg)?;
    task_manager::list_tasks(tm, std::path::Path::new(&repo_path))
}

#[tauri::command]
pub fn fire_task_notify_hook(
    session_id: String,
    db_state: State<DbState>,
    config_state: State<ConfigState>,
) -> Result<(), String> {
    let conn = db_state.0.lock().map_err(|e| e.to_string())?;
    let session = db::get_session(&conn, &session_id)
        .map_err(|e| e.to_string())?
        .ok_or("session not found")?;
    let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
    if session.task_key.is_some() {
        if let Some(cwd) = session_cwd(&conn, &session) {
            fire_task_hook(&cfg, &session, "on_notify", &cwd);
        }
    }
    poll_pr_for_session(&conn, &cfg, &session)?;
    Ok(())
}

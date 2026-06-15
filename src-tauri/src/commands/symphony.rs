use tauri::State;

use crate::state::SymphonyHandle;

#[tauri::command]
pub fn get_symphony_status(
    state: State<SymphonyHandle>,
    db_state: State<crate::DbState>,
    config_state: State<crate::ConfigState>,
) -> Result<String, String> {
    let symphony = state.0.lock().map_err(|e| e.to_string())?;
    let active = symphony.is_running();
    if !active {
        return Ok("{\"active\":false,\"slots_used\":0,\"max_concurrent\":0}".to_string());
    }
    let conn = db_state.0.lock().map_err(|e| e.to_string())?;
    let slots_used: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sessions WHERE auto_dispatched = 1 AND status = 'active'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);
    let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
    let max_concurrent: usize = cfg
        .task_management
        .as_ref()
        .and_then(|tm| tm.auto_dispatch.as_ref())
        .map(|ad| ad.max_concurrent)
        .unwrap_or(3);
    Ok(format!(
        "{{\"active\":true,\"slots_used\":{slots_used},\"max_concurrent\":{max_concurrent}}}"
    ))
}

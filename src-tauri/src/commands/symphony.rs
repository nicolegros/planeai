use tauri::State;

use crate::state::SymphonyHandle;

#[tauri::command]
pub fn get_symphony_status(state: State<SymphonyHandle>) -> Result<String, String> {
    let symphony = state.0.lock().map_err(|e| e.to_string())?;
    let active = symphony.is_running();
    Ok(format!("{{\"active\":{active}}}"))
}

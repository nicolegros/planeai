use tauri::ipc::Channel;
use tauri::State;

use crate::lsp::{LspConnection, LspState};
use crate::state::ConfigState;

#[tauri::command]
pub async fn lsp_connect(
    repo_path: String,
    file_path: String,
    on_message: Channel<String>,
    config_state: State<'_, ConfigState>,
    state: State<'_, LspState>,
) -> Result<LspConnection, String> {
    let config = config_state
        .0
        .lock()
        .map_err(|error| error.to_string())?
        .clone();
    let manager = state.0.clone();
    let result = {
        let mut guard = manager.lock().await;
        guard
            .connect(repo_path, file_path, &config, on_message, manager.clone())
            .await
    };
    result
}

#[tauri::command]
pub async fn lsp_send(
    connection_id: String,
    message: String,
    state: State<'_, LspState>,
) -> Result<(), String> {
    state.0.lock().await.send(&connection_id, &message).await
}

#[tauri::command]
pub async fn lsp_disconnect(
    connection_id: String,
    state: State<'_, LspState>,
) -> Result<(), String> {
    state.0.lock().await.disconnect(&connection_id).await;
    Ok(())
}

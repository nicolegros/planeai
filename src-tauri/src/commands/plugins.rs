use serde_json::Value;
use tauri::{AppHandle, Emitter, State};

use crate::plugins::{PluginInventory, PluginRuntimeHandle};

#[tauri::command]
pub async fn list_plugins(
    runtime: State<'_, PluginRuntimeHandle>,
) -> Result<Vec<PluginInventory>, String> {
    runtime.0.list().await
}

#[tauri::command]
pub async fn install_local_plugin(
    source_path: String,
    runtime: State<'_, PluginRuntimeHandle>,
) -> Result<PluginInventory, String> {
    runtime.0.install_local(source_path).await
}

#[tauri::command]
pub async fn remove_local_plugin(
    plugin_id: String,
    runtime: State<'_, PluginRuntimeHandle>,
) -> Result<(), String> {
    runtime.0.remove(&plugin_id).await
}

#[tauri::command]
pub async fn plugin_call(
    plugin_id: String,
    method: String,
    params: Value,
    runtime: State<'_, PluginRuntimeHandle>,
) -> Result<Value, String> {
    runtime.0.call(&plugin_id, &method, params).await
}

#[tauri::command]
pub async fn plugin_data_changed(plugin_id: String, app: AppHandle) -> Result<(), String> {
    app.emit("plugin-data-changed", plugin_id)
        .map_err(|error| format!("failed to emit plugin data change: {error}"))
}

#[tauri::command]
pub async fn plugin_settings(
    plugin_id: String,
    runtime: State<'_, PluginRuntimeHandle>,
) -> Result<Value, String> {
    runtime.0.settings(&plugin_id).await
}

#[tauri::command]
pub async fn update_plugin_settings(
    plugin_id: String,
    settings: Value,
    runtime: State<'_, PluginRuntimeHandle>,
) -> Result<Value, String> {
    runtime.0.update_settings(&plugin_id, settings).await
}

#[tauri::command]
pub async fn local_plugin_ui_source(
    plugin_id: String,
    contribution_id: String,
    runtime: State<'_, PluginRuntimeHandle>,
) -> Result<String, String> {
    runtime
        .0
        .local_ui_source(&plugin_id, &contribution_id)
        .await
}
#[tauri::command]
pub async fn enable_plugin(
    plugin_id: String,
    runtime: State<'_, PluginRuntimeHandle>,
) -> Result<PluginInventory, String> {
    runtime.0.enable(&plugin_id).await
}

#[tauri::command]
pub async fn disable_plugin(
    plugin_id: String,
    runtime: State<'_, PluginRuntimeHandle>,
) -> Result<PluginInventory, String> {
    runtime.0.disable(&plugin_id).await
}

#[tauri::command]
pub async fn reload_plugin(
    plugin_id: String,
    runtime: State<'_, PluginRuntimeHandle>,
) -> Result<PluginInventory, String> {
    runtime.0.reload(&plugin_id).await
}

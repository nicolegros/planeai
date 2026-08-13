use serde_json::Value;
use tauri::State;
use tauri_plugin_opener::OpenerExt;

use crate::plugins::{JiraPluginStatus, PluginInventory, PluginRuntimeHandle};

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

/// Plugin-scoped status bridge. This intentionally exposes only Jira's fixed
/// read-only method rather than accepting an arbitrary RPC method name.
#[tauri::command]
pub async fn jira_plugin_status(
    plugin_id: String,
    runtime: State<'_, PluginRuntimeHandle>,
) -> Result<JiraPluginStatus, String> {
    runtime.0.jira_status(&plugin_id).await
}

/// Open the Atlassian authorization page for the bundled Jira plugin. This is
/// intentionally not a generic plugin shell or Tauri capability.
#[tauri::command]
pub async fn open_jira_authorization_url(url: String, app: tauri::AppHandle) -> Result<(), String> {
    if !url.starts_with("https://auth.atlassian.com/authorize?") {
        return Err("Jira authorization URL must target auth.atlassian.com".to_string());
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|error| format!("failed to open browser for Jira authorization: {error}"))
}

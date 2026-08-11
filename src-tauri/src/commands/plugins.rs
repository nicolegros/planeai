use tauri::State;

use crate::plugins::{JiraPluginStatus, PluginInventory, PluginRuntimeHandle};

#[tauri::command]
pub async fn list_plugins(
    runtime: State<'_, PluginRuntimeHandle>,
) -> Result<Vec<PluginInventory>, String> {
    runtime.0.list().await
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

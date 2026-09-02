use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::state::{ConfigState, DbState};

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

#[tauri::command]
pub async fn jira_migration_status(
    config: State<'_, ConfigState>,
    db: State<'_, DbState>,
) -> Result<crate::jira_migration::JiraMigrationStatus, String> {
    let config = config.0.lock().map_err(|error| error.to_string())?.clone();
    let db = db.0.clone();
    crate::commands::blocking(move || {
        let conn = db.lock().map_err(|error| error.to_string())?;
        crate::jira_migration::status(&conn, &config)
    })
    .await
}

#[tauri::command]
pub async fn migrate_legacy_jira(
    app: AppHandle,
    config: State<'_, ConfigState>,
    db: State<'_, DbState>,
    runtime: State<'_, PluginRuntimeHandle>,
) -> Result<crate::jira_migration::JiraMigrationStatus, String> {
    let mut config_snapshot = config.0.lock().map_err(|error| error.to_string())?.clone();
    let config_dir = crate::config::config_dir(&app.package_info().name);
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("failed to resolve PlaneAI app data directory: {error}"))?;
    let db_conn = db.0.clone();
    let import_app_data_dir = app_data_dir.clone();
    let (imported_config, import_status) = crate::commands::blocking(move || {
        let conn = db_conn.lock().map_err(|error| error.to_string())?;
        let status = crate::jira_migration::import(
            &conn,
            &config_dir,
            &import_app_data_dir,
            &mut config_snapshot,
        )?;
        Ok((config_snapshot, status))
    })
    .await?;

    if import_status.state == crate::jira_migration::JiraMigrationState::Completed {
        return Ok(import_status);
    }

    // Only publish the persisted config after the legacy snapshot has been
    // successfully imported and validated. A later enable failure remains
    // retryable from the migration backup without reviving legacy ownership.
    *config.0.lock().map_err(|error| error.to_string())? = imported_config.clone();
    if let Err(error) = runtime.0.enable_jira_after_migration().await {
        let db = db.0.clone();
        let failure = error.clone();
        crate::commands::blocking(move || {
            let conn = db.lock().map_err(|lock| lock.to_string())?;
            crate::jira_migration::mark_failed(&conn, &failure)
        })
        .await?;
        return Err(error);
    }
    let db = db.0.clone();
    let completed = crate::commands::blocking(move || {
        let conn = db.lock().map_err(|error| error.to_string())?;
        crate::jira_migration::mark_completed(&conn, &app_data_dir)?;
        crate::jira_migration::status(&conn, &imported_config)
    })
    .await?;
    app.emit("jira-migration-changed", &completed)
        .map_err(|error| format!("failed to emit Jira migration update: {error}"))?;
    Ok(completed)
}

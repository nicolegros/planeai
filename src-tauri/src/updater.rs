use tauri::{AppHandle, Emitter, State};
use tauri_plugin_updater::UpdaterExt;

use crate::plugins::PluginRuntimeHandle;

#[derive(Clone, serde::Serialize)]
pub struct UpdateInfo {
    version: String,
    body: Option<String>,
}

pub fn check_for_updates(app: &AppHandle) {
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = do_check(&handle).await {
            tracing::warn!("update check failed: {e}");
        }
    });
}

async fn lookup_update(app: &AppHandle) -> anyhow::Result<Option<UpdateInfo>> {
    let updater = app.updater()?.check().await?;
    Ok(updater.map(|update| UpdateInfo {
        version: update.version.clone(),
        body: update.body.clone(),
    }))
}

async fn do_check(app: &AppHandle) -> anyhow::Result<()> {
    match lookup_update(app).await? {
        Some(update) => {
            tracing::info!("update available: {}", update.version);
            let _ = app.emit("update-available", update);
        }
        None => tracing::info!("app is up to date"),
    }
    Ok(())
}

#[tauri::command]
pub async fn get_app_version(app: AppHandle) -> String {
    app.package_info().version.to_string()
}

#[tauri::command]
pub async fn check_for_update(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
    lookup_update(&app).await.map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn install_update(
    app: AppHandle,
    runtime: State<'_, PluginRuntimeHandle>,
) -> Result<(), String> {
    let update = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?;

    let update = update.ok_or_else(|| "No update available".to_string())?;
    let bytes = update
        .download(|_, _| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    if !runtime.0.begin_shutdown() {
        return Err("plugin runtime is already shutting down".to_string());
    }
    let enabled_plugin_ids = match runtime.0.shutdown_for_update().await {
        Ok(plugin_ids) => plugin_ids,
        Err(error) => return Err(error),
    };
    let install_result =
        crate::commands::blocking(move || update.install(bytes).map_err(|e| e.to_string())).await;
    if let Err(error) = install_result {
        runtime
            .0
            .restore_after_failed_update(&enabled_plugin_ids)
            .await;
        return Err(error);
    }
    app.restart();
}

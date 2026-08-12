use tauri::{AppHandle, Emitter, State};
use tauri_plugin_updater::UpdaterExt;

use crate::plugins::PluginRuntimeHandle;

#[derive(Clone, serde::Serialize)]
struct UpdateAvailablePayload {
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

async fn do_check(app: &AppHandle) -> anyhow::Result<()> {
    let updater = app.updater()?.check().await?;
    if let Some(update) = updater {
        tracing::info!("update available: {}", update.version);
        let _ = app.emit(
            "update-available",
            UpdateAvailablePayload {
                version: update.version.clone(),
                body: update.body.clone(),
            },
        );
    } else {
        tracing::info!("app is up to date");
    }
    Ok(())
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

//! Process-wide access to PlaneAI's private rmux daemon.
//!
//! The client is created on first use and shared. It is **not** assumed to stay
//! usable: the daemon exits with its last session, and an idle connection can be
//! closed underneath us, so a cached client can be dead by the time the next
//! operation runs (ADR-0012). Every operation therefore goes through
//! [`with_retry`], which reconnects and tries again once when the transport has
//! gone away.

use std::sync::Arc;

use planeai_rmux::{RmuxClient, RmuxConfig};
use tokio::sync::Mutex;

static CLIENT: tokio::sync::OnceCell<Mutex<Option<Arc<RmuxClient>>>> =
    tokio::sync::OnceCell::const_new();

/// Resolve the endpoint and bundled daemon binary for this installation.
fn config(app: Option<&tauri::AppHandle>) -> RmuxConfig {
    // Share the runtime directory with the existing daemon socket so both
    // backends inherit the same location and permissions.
    let runtime_dir = planeai_ipc::daemon_socket_path()
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(std::env::temp_dir);
    let config = RmuxConfig::app_private(&runtime_dir);
    match app.map(crate::paths::resolve_rmux_daemon_binary) {
        Some(binary) if binary.exists() => config.with_daemon_binary(binary),
        // Without a bundled sidecar the SDK falls back to PATH, which is what a
        // development machine with rmux installed relies on.
        _ => config,
    }
}

/// Get the shared client, preferring a bundled sidecar resolved from `app`.
///
/// Prefer [`with_retry`] over calling this directly: a cached client can be dead
/// by the time it is used.
pub async fn client_with(app: Option<&tauri::AppHandle>) -> Result<Arc<RmuxClient>, String> {
    let cell = CLIENT.get_or_init(|| async { Mutex::new(None) }).await;
    let mut guard = cell.lock().await;
    if let Some(existing) = guard.as_ref() {
        return Ok(existing.clone());
    }

    let client = RmuxClient::connect(config(app))
        .await
        .map_err(|error| error.to_string())?;
    let client = Arc::new(client);
    *guard = Some(client.clone());
    Ok(client)
}

/// Drop the cached connection so the next call reconnects and restarts the daemon.
pub async fn invalidate() {
    if let Some(cell) = CLIENT.get() {
        *cell.lock().await = None;
    }
}

/// Run an rmux operation, reconnecting once if the transport has gone away.
///
/// A cached client dies for ordinary reasons — the daemon exits with its last
/// session, or an idle connection is closed — and the failure only surfaces when
/// the next operation runs. Retrying here keeps every call site free of that
/// concern instead of each one rediscovering it.
pub async fn with_retry<T, Operation, Fut>(operation: Operation) -> Result<T, String>
where
    Operation: Fn(Arc<RmuxClient>) -> Fut,
    Fut: std::future::Future<Output = planeai_rmux::Result<T>>,
{
    with_retry_for(None, operation).await
}

/// As [`with_retry`], preferring a bundled sidecar resolved from `app`.
pub async fn with_retry_for<T, Operation, Fut>(
    app: Option<&tauri::AppHandle>,
    operation: Operation,
) -> Result<T, String>
where
    Operation: Fn(Arc<RmuxClient>) -> Fut,
    Fut: std::future::Future<Output = planeai_rmux::Result<T>>,
{
    let client = client_with(app).await?;
    match operation(client).await {
        Ok(value) => Ok(value),
        Err(error) if error.is_daemon_gone() => {
            tracing::info!(%error, "rmux transport was gone; reconnecting and retrying once");
            invalidate().await;
            let client = client_with(app).await?;
            operation(client).await.map_err(|error| error.to_string())
        }
        Err(error) => Err(error.to_string()),
    }
}

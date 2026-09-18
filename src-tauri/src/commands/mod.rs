pub mod cleanup;
pub mod cli;
pub mod config;
pub mod editor;
pub mod file_explorer;
pub mod files;
pub mod git;
pub mod loops;
pub mod lsp;
pub mod notify;
pub mod plugins;
pub mod projects;
pub mod sessions;
pub mod symphony;
pub mod tasks;

pub use cleanup::*;
pub use cli::*;
pub use config::*;
pub use editor::*;
pub use file_explorer::*;
pub use files::*;
pub use git::*;
pub use loops::*;
pub use lsp::*;
pub use notify::*;
pub use plugins::*;
pub use projects::*;
pub use sessions::*;
pub use symphony::*;
pub use tasks::*;

/// Run blocking work off the main thread. Use for any Tauri command that
/// performs I/O (subprocesses, disk, network) to avoid stalling the WebView
/// event loop on macOS.
pub(crate) async fn blocking<F, T>(f: F) -> Result<T, String>
where
    F: FnOnce() -> Result<T, String> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| format!("task failed: {e}"))?
}

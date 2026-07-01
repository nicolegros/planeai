pub mod ci;
pub mod cleanup;
pub mod cli;
pub mod config;
pub mod file_explorer;
pub mod files;
pub mod git;
pub mod notify;
pub mod pr;
pub mod pr_comments;
pub mod projects;
pub mod sessions;
pub mod symphony;
pub mod tasks;

pub use ci::get_ci_failure_logs;
pub use cleanup::*;
pub use cli::*;
pub use config::*;
pub use file_explorer::*;
pub use files::*;
pub use git::*;
pub use notify::*;
pub use pr::{
    create_pr, fetch_pr_url, generate_pr_defaults, get_allowed_merge_strategies, get_ci_checks,
    get_merge_conflict_status, get_merge_state, get_pr_status, mark_pr_ready, merge_pr,
};
pub use pr_comments::get_pr_comments;
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

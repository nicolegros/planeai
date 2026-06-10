pub mod command;
pub mod dispatch;
pub mod orchestrator;
pub mod session;
pub mod task;
pub mod template;

use std::path::PathBuf;

const APP_ID: &str = "ca.nicolegros.planeai";

pub fn app_data_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").unwrap_or_default();
        PathBuf::from(home)
            .join("Library/Application Support")
            .join(APP_ID)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let base = std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| {
            format!("{}/.local/share", std::env::var("HOME").unwrap_or_default())
        });
        PathBuf::from(base).join(APP_ID)
    }
}

pub fn notify_socket_path() -> PathBuf {
    app_data_dir().join("notify.sock")
}

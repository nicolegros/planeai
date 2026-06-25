use std::path::PathBuf;

const APP_ID: &str = "ca.nicolegros.planeai";

fn home_dir() -> String {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default()
}

/// Returns the platform-specific app data directory.
pub fn app_data_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        PathBuf::from(home_dir())
            .join("Library/Application Support")
            .join(APP_ID)
    }
    #[cfg(target_os = "windows")]
    {
        let base = std::env::var("APPDATA")
            .unwrap_or_else(|_| format!("{}\\AppData\\Roaming", home_dir()));
        PathBuf::from(base).join(APP_ID)
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        let base = std::env::var("XDG_DATA_HOME")
            .unwrap_or_else(|_| format!("{}/.local/share", home_dir()));
        PathBuf::from(base).join(APP_ID)
    }
}

pub fn db_path() -> PathBuf {
    app_data_dir().join("planeai.db")
}

pub fn notify_socket_path() -> PathBuf {
    app_data_dir().join("notify.sock")
}

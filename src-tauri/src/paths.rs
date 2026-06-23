use std::path::PathBuf;

use crate::config::home_dir;

const APP_ID: &str = "ca.nicolegros.planeai";

/// Returns the app data directory (same as Tauri's app_data_dir).
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

#[allow(dead_code)]
pub fn db_path() -> PathBuf {
    app_data_dir().join("planeai.db")
}

#[allow(dead_code)]
pub fn notify_socket_path() -> PathBuf {
    app_data_dir().join("notify.sock")
}

/// Resolve the daemon binary path. In production, use the bundled resource.
/// In development, use the workspace target directory.
pub fn resolve_daemon_binary(app: &tauri::AppHandle) -> PathBuf {
    use tauri::Manager as _;

    let bin_name = if cfg!(windows) {
        "planeai-daemon.exe"
    } else {
        "planeai-daemon"
    };

    // Try resource dir (production bundle — externalBin places it here)
    if let Ok(resource_dir) = app.path().resource_dir() {
        let bundled = resource_dir.join(bin_name);
        if bundled.exists() {
            return bundled;
        }
    }

    // Fallback: same directory as the main executable (workspace target/debug or target/release)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let sibling = dir.join(bin_name);
            if sibling.exists() {
                return sibling;
            }
        }
    }

    // Last resort: hope it's on PATH
    tracing::warn!("daemon binary not found alongside executable, falling back to PATH");
    PathBuf::from(bin_name)
}

/// Resolve the daemon binary without an AppHandle (for use in sync contexts).
pub fn resolve_daemon_binary_fallback() -> PathBuf {
    let bin_name = if cfg!(windows) {
        "planeai-daemon.exe"
    } else {
        "planeai-daemon"
    };

    // Same directory as current executable (works in dev and production bundles)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let sibling = dir.join(bin_name);
            if sibling.exists() {
                return sibling;
            }
            // macOS bundle: exe is in <App>.app/Contents/MacOS/, Resources is sibling
            let resources = dir.parent().map(|p| p.join("Resources").join(bin_name));
            if let Some(ref bundled) = resources {
                if bundled.exists() {
                    return bundled.clone();
                }
            }
        }
    }

    // Check /usr/local/bin (symlink from install)
    let symlinked = PathBuf::from("/usr/local/bin").join(bin_name);
    if symlinked.exists() {
        return symlinked;
    }

    PathBuf::from(bin_name)
}

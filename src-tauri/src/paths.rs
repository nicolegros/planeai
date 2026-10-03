use std::path::PathBuf;

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

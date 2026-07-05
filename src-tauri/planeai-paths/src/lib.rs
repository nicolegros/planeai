use std::path::PathBuf;

const APP_ID: &str = "ca.nicolegros.planeai";

pub fn home_dir() -> String {
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

/// Raise the process file descriptor soft limit to min(hard_limit, 10240).
///
/// On macOS the default soft limit is 256, which is far too low for processes
/// managing WebView FDs, PTY sessions, IPC sockets, and log files.
/// Call this early in main() before any I/O.
#[cfg(unix)]
pub fn raise_fd_limit() {
    unsafe {
        let mut rlim = std::mem::MaybeUninit::<libc::rlimit>::zeroed().assume_init();
        if libc::getrlimit(libc::RLIMIT_NOFILE, &mut rlim) == 0 {
            let target = rlim.rlim_max.min(10240);
            if rlim.rlim_cur < target {
                rlim.rlim_cur = target;
                let _ = libc::setrlimit(libc::RLIMIT_NOFILE, &rlim);
            }
        }
    }
}

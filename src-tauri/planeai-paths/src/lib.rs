use std::path::{Path, PathBuf};

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

/// Atomically replace `path` with `temporary`. The temporary file must be in
/// the destination directory and is consumed on success.
#[cfg(not(windows))]
pub fn replace_file_atomically(temporary: &Path, path: &Path) -> std::io::Result<()> {
    std::fs::rename(temporary, path)
}

/// Atomically replace `path` with `temporary`. Windows cannot rename over an
/// existing file, so use `ReplaceFileW` when the destination already exists.
#[cfg(windows)]
pub fn replace_file_atomically(temporary: &Path, path: &Path) -> std::io::Result<()> {
    match std::fs::metadata(path) {
        Ok(_) => replace_existing_file_windows(path, temporary),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::rename(temporary, path)
        }
        Err(error) => Err(error),
    }
}

#[cfg(windows)]
fn replace_existing_file_windows(path: &Path, temporary: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt as _;

    #[link(name = "Kernel32")]
    extern "system" {
        fn ReplaceFileW(
            replaced_file_name: *const u16,
            replacement_file_name: *const u16,
            backup_file_name: *const u16,
            replace_flags: u32,
            exclude: *mut std::ffi::c_void,
            reserved: *mut std::ffi::c_void,
        ) -> i32;
    }

    let path = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let temporary = temporary
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    if unsafe {
        ReplaceFileW(
            path.as_ptr(),
            temporary.as_ptr(),
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    } == 0
    {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_replacement_overwrites_an_existing_file() {
        let root = std::env::temp_dir().join(format!(
            "planeai-paths-replace-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("settings.json");
        let temporary = root.join(".settings.tmp");
        std::fs::write(&path, "old").unwrap();
        std::fs::write(&temporary, "new").unwrap();
        replace_file_atomically(&temporary, &path).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
        assert!(!temporary.exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}

//! Cross-platform IPC abstraction over Unix sockets / Windows named pipes.
//!
//! Enable the `async` feature for tokio-based async listener/stream types.

use std::path::{Path, PathBuf};

pub mod sync_impl;
pub use sync_impl::{connect, IpcListener, IpcStream};

#[cfg(feature = "async")]
pub mod r#async;

/// Named IPC channels used by planeai components.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Notify,
    Symphony,
    Daemon,
}

impl Channel {
    pub fn name(&self) -> &'static str {
        match self {
            Channel::Notify => "notify",
            Channel::Symphony => "symphony",
            Channel::Daemon => "daemon",
        }
    }
}

/// Returns the platform-specific address/path for a channel.
pub fn address(channel: Channel, app_dir: &Path) -> String {
    socket_path(channel, app_dir).to_string_lossy().into_owned()
}

/// Check whether the socket/pipe exists on disk.
pub fn channel_exists(channel: Channel, app_dir: &Path) -> bool {
    #[cfg(unix)]
    {
        socket_path(channel, app_dir).exists()
    }
    #[cfg(windows)]
    {
        let _ = (channel, app_dir);
        true // Named pipes don't have filesystem presence
    }
}

/// Returns the socket/pipe path for a channel.
pub fn socket_path(channel: Channel, app_dir: &Path) -> PathBuf {
    #[cfg(unix)]
    {
        if channel == Channel::Daemon {
            daemon_socket_path()
        } else {
            app_dir.join(format!("{}.sock", channel.name()))
        }
    }
    #[cfg(windows)]
    {
        let _ = app_dir;
        if channel == Channel::Daemon {
            PathBuf::from(r"\\.\pipe\planeai-daemon")
        } else {
            PathBuf::from(format!(r"\\.\pipe\planeai-{}", channel.name()))
        }
    }
}

/// Returns the default daemon socket path per platform conventions.
pub fn daemon_socket_path() -> PathBuf {
    #[cfg(unix)]
    {
        daemon_runtime_dir().join("daemon.sock")
    }
    #[cfg(windows)]
    {
        PathBuf::from(r"\\.\pipe\planeai-daemon")
    }
}

/// The directory holding the PTY and rmux daemon sockets. A process whose `HOME` is not the
/// account's home, such as an app run with an isolated data folder, gets its own, since the
/// daemons' sessions belong to one database and each app closes panes its database does not own.
#[cfg(unix)]
pub fn daemon_runtime_dir() -> &'static Path {
    static DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        let home = std::env::var_os("HOME");
        runtime_dir(
            std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from),
            unsafe { libc::getuid() },
            home.as_deref().map(Path::new),
            account_home().as_deref(),
        )
    })
}

#[cfg(unix)]
fn runtime_dir(
    xdg_runtime_dir: Option<PathBuf>,
    uid: u32,
    home: Option<&Path>,
    account_home: Option<&Path>,
) -> PathBuf {
    let resolved = |path: &Path| std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    // An unknown account home keeps the shared directory, so a failed lookup never strands
    // the daemons of a normal install.
    let suffix = match (home.map(resolved), account_home.map(resolved)) {
        (Some(home), Some(account_home)) if home != account_home => {
            format!("-{:016x}", fnv1a(&home))
        }
        _ => String::new(),
    };
    match xdg_runtime_dir {
        Some(dir) => dir.join(format!("planeai{suffix}")),
        None => PathBuf::from(format!("/tmp/planeai-{uid}{suffix}")),
    }
}

/// A hash that stays the same across builds, unlike `std`'s hasher.
#[cfg(unix)]
fn fnv1a(path: &Path) -> u64 {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str()
        .as_bytes()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
        })
}

#[cfg(unix)]
fn account_home() -> Option<PathBuf> {
    use std::ffi::CStr;
    use std::os::unix::ffi::OsStrExt;
    let mut passwd = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut buffer = vec![0 as libc::c_char; 4096];
    let mut result = std::ptr::null_mut();
    loop {
        // SAFETY: getpwuid_r writes into `passwd` and `buffer`, which outlive the read below.
        let status = unsafe {
            libc::getpwuid_r(
                libc::getuid(),
                passwd.as_mut_ptr(),
                buffer.as_mut_ptr(),
                buffer.len(),
                &mut result,
            )
        };
        match status {
            0 if !result.is_null() => break,
            libc::ERANGE if buffer.len() < 1 << 20 => buffer.resize(buffer.len() * 4, 0),
            _ => return None,
        }
    }
    // SAFETY: on success `pw_dir` points to a NUL-terminated string inside `buffer`.
    let dir = unsafe { CStr::from_ptr((*result).pw_dir) };
    Some(PathBuf::from(std::ffi::OsStr::from_bytes(dir.to_bytes())))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn the_account_home_keeps_the_shared_runtime_dir() {
        let home = Path::new("/Users/me");
        assert_eq!(
            runtime_dir(None, 501, Some(home), Some(home)),
            PathBuf::from("/tmp/planeai-501")
        );
        assert_eq!(
            runtime_dir(None, 501, None, Some(home)),
            PathBuf::from("/tmp/planeai-501")
        );
        assert_eq!(
            runtime_dir(None, 501, Some(Path::new("/tmp/e2e")), None),
            PathBuf::from("/tmp/planeai-501"),
            "an unknown account home keeps the shared directory"
        );
        assert_eq!(
            runtime_dir(Some("/run/user/501".into()), 501, Some(home), Some(home)),
            PathBuf::from("/run/user/501/planeai")
        );
    }

    #[test]
    fn another_spelling_of_the_account_home_keeps_the_shared_runtime_dir() {
        let account = tempfile::tempdir().unwrap();
        let link = tempfile::tempdir().unwrap();
        let linked_home = link.path().join("home");
        std::os::unix::fs::symlink(account.path(), &linked_home).unwrap();
        let trailing_slash = PathBuf::from(format!("{}/", account.path().display()));
        for home in [linked_home, trailing_slash] {
            assert_eq!(
                runtime_dir(None, 501, Some(&home), Some(account.path())),
                PathBuf::from("/tmp/planeai-501")
            );
        }
    }

    #[test]
    fn every_spelling_of_another_home_finds_the_same_daemons() {
        let isolated = tempfile::tempdir().unwrap();
        let link = tempfile::tempdir().unwrap();
        let linked_home = link.path().join("home");
        std::os::unix::fs::symlink(isolated.path(), &linked_home).unwrap();
        let trailing_slash = PathBuf::from(format!("{}/", isolated.path().display()));
        let account = Some(Path::new("/Users/me"));
        let dir = runtime_dir(None, 501, Some(isolated.path()), account);
        for home in [linked_home, trailing_slash] {
            assert_eq!(runtime_dir(None, 501, Some(&home), account), dir);
        }
    }

    #[test]
    fn another_home_gets_its_own_short_runtime_dir() {
        let account = Path::new("/Users/me");
        let isolated = runtime_dir(None, 501, Some(Path::new("/tmp/e2e")), Some(account));
        assert_ne!(isolated, PathBuf::from("/tmp/planeai-501"));
        assert!(isolated.to_string_lossy().starts_with("/tmp/planeai-501-"));
        assert_eq!(
            isolated,
            runtime_dir(None, 501, Some(Path::new("/tmp/e2e")), Some(account)),
            "the same home always finds the same daemons"
        );
        assert_ne!(
            isolated,
            runtime_dir(None, 501, Some(Path::new("/tmp/e2e2")), Some(account))
        );
        // Unix socket paths are limited to about 104 bytes.
        assert!(isolated.join("daemon.sock").as_os_str().len() < 60);
        assert_eq!(
            runtime_dir(
                Some("/run/user/501".into()),
                501,
                Some(Path::new("/tmp/e2e")),
                Some(account)
            )
            .parent(),
            Some(Path::new("/run/user/501"))
        );
    }

    #[test]
    fn this_process_resolves_the_account_home() {
        assert!(account_home().is_some_and(|home| home.is_absolute()));
    }
}

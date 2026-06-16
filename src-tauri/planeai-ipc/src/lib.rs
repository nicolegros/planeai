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
        if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
            PathBuf::from(dir).join("planeai").join("daemon.sock")
        } else {
            let uid = unsafe { libc::getuid() };
            PathBuf::from(format!("/tmp/planeai-{uid}")).join("daemon.sock")
        }
    }
    #[cfg(windows)]
    {
        PathBuf::from(r"\\.\pipe\planeai-daemon")
    }
}

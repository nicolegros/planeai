//! Cross-platform IPC abstraction over Unix sockets / Windows named pipes.

#[allow(unused_imports)]
pub use crate::ipc_platform::{channel_exists, connect, IpcListener, IpcStream};

use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Notify,
    Symphony,
}

impl Channel {
    /// Returns the base name used for socket files (Unix) or pipe suffix (Windows).
    pub fn socket_name(&self) -> &'static str {
        match self {
            Channel::Notify => "notify",
            Channel::Symphony => "symphony",
        }
    }
}

/// Returns the platform-specific address string for a channel.
/// On Unix this is the socket file path; on Windows the named pipe path.
pub fn channel_address(channel: Channel, app_dir: &Path) -> String {
    #[cfg(unix)]
    {
        app_dir
            .join(format!("{}.sock", channel.socket_name()))
            .to_string_lossy()
            .into_owned()
    }
    #[cfg(windows)]
    {
        let _ = app_dir;
        format!(r"\\.\pipe\planeai-{}", channel.socket_name())
    }
}

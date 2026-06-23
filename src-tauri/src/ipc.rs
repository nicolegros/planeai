//! Minimal IPC module for the notify socket (replaces planeai-ipc for notify channel only).

use std::path::Path;

#[derive(Debug, Clone, Copy)]
pub enum Channel {
    Notify,
    Symphony,
}

/// Get the socket path for a channel.
pub fn address(channel: Channel, app_dir: &Path) -> String {
    match channel {
        Channel::Notify => app_dir.join("notify.sock").to_string_lossy().to_string(),
        Channel::Symphony => app_dir.join("symphony.sock").to_string_lossy().to_string(),
    }
}

/// Check if a channel's socket exists.
pub fn channel_exists(channel: Channel, app_dir: &Path) -> bool {
    let path = address(channel, app_dir);
    Path::new(&path).exists()
}

/// Connect to a channel socket.
#[cfg(not(windows))]
pub fn connect(channel: Channel, app_dir: &Path) -> std::io::Result<std::os::unix::net::UnixStream> {
    let path = address(channel, app_dir);
    std::os::unix::net::UnixStream::connect(path)
}

#[cfg(windows)]
pub fn connect(channel: Channel, app_dir: &Path) -> std::io::Result<std::fs::File> {
    let path = address(channel, app_dir);
    std::fs::OpenOptions::new().read(true).write(true).open(path)
}

/// Listener for incoming IPC connections.
pub struct IpcListener {
    #[cfg(not(windows))]
    inner: std::os::unix::net::UnixListener,
    #[cfg(windows)]
    #[allow(dead_code)]
    path: String,
}

impl IpcListener {
    pub fn bind(channel: Channel, app_dir: &Path) -> std::io::Result<Self> {
        let path = address(channel, app_dir);
        // Remove stale socket file
        let _ = std::fs::remove_file(&path);
        #[cfg(not(windows))]
        {
            let listener = std::os::unix::net::UnixListener::bind(&path)?;
            Ok(Self { inner: listener })
        }
        #[cfg(windows)]
        {
            // On Windows, use a named pipe or file-based approach
            Ok(Self { path: path })
        }
    }

    #[cfg(not(windows))]
    pub fn accept(&self) -> std::io::Result<std::os::unix::net::UnixStream> {
        let (stream, _) = self.inner.accept()?;
        Ok(stream)
    }

    #[cfg(windows)]
    pub fn accept(&self) -> std::io::Result<std::fs::File> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "IPC listener not implemented on Windows",
        ))
    }
}

#[cfg(not(windows))]
pub type IpcStream = std::os::unix::net::UnixStream;

#[cfg(windows)]
pub type IpcStream = std::fs::File;

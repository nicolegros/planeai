use std::io::{Read, Write};
use std::os::unix::net::{UnixListener as StdUnixListener, UnixStream as StdUnixStream};
use std::path::{Path, PathBuf};

use crate::ipc::Channel;

pub struct IpcListener {
    inner: StdUnixListener,
}

pub struct IpcStream {
    inner: StdUnixStream,
}

impl IpcListener {
    pub fn bind(channel: Channel, app_dir: &Path) -> Result<Self, String> {
        let path = socket_path(channel, app_dir);
        let _ = std::fs::remove_file(&path);
        let inner = StdUnixListener::bind(&path).map_err(|e| format!("bind failed: {e}"))?;
        Ok(Self { inner })
    }

    pub fn accept(&self) -> Result<IpcStream, String> {
        let (stream, _) = self
            .inner
            .accept()
            .map_err(|e| format!("accept failed: {e}"))?;
        Ok(IpcStream { inner: stream })
    }
}

/// Returns the platform-specific address string for a channel.
pub fn address(channel: Channel, app_dir: &Path) -> String {
    socket_path(channel, app_dir).to_string_lossy().into_owned()
}

pub fn connect(channel: Channel, app_dir: &Path) -> Result<IpcStream, String> {
    let path = socket_path(channel, app_dir);
    let inner = StdUnixStream::connect(&path).map_err(|e| format!("connect failed: {e}"))?;
    Ok(IpcStream { inner })
}

pub fn channel_exists(channel: Channel, app_dir: &Path) -> bool {
    socket_path(channel, app_dir).exists()
}

fn socket_path(channel: Channel, app_dir: &Path) -> PathBuf {
    app_dir.join(format!("{}.sock", channel.name()))
}

impl Read for IpcStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buf)
    }
}

impl Write for IpcStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

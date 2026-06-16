//! Async (tokio) IPC listener and stream.

use std::path::Path;
use tokio::io::{AsyncRead, AsyncWrite};

/// Async IPC stream wrapping platform transport.
pub struct AsyncIpcStream {
    inner: InnerStream,
}

enum InnerStream {
    #[cfg(unix)]
    Unix(tokio::net::UnixStream),
    #[cfg(windows)]
    _Placeholder(std::convert::Infallible),
}

impl AsyncRead for AsyncIpcStream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match &mut self.get_mut().inner {
            #[cfg(unix)]
            InnerStream::Unix(s) => std::pin::Pin::new(s).poll_read(cx, buf),
            #[cfg(windows)]
            InnerStream::_Placeholder(never) => match *never {},
        }
    }
}

impl AsyncWrite for AsyncIpcStream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        match &mut self.get_mut().inner {
            #[cfg(unix)]
            InnerStream::Unix(s) => std::pin::Pin::new(s).poll_write(cx, buf),
            #[cfg(windows)]
            InnerStream::_Placeholder(never) => match *never {},
        }
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match &mut self.get_mut().inner {
            #[cfg(unix)]
            InnerStream::Unix(s) => std::pin::Pin::new(s).poll_flush(cx),
            #[cfg(windows)]
            InnerStream::_Placeholder(never) => match *never {},
        }
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match &mut self.get_mut().inner {
            #[cfg(unix)]
            InnerStream::Unix(s) => std::pin::Pin::new(s).poll_shutdown(cx),
            #[cfg(windows)]
            InnerStream::_Placeholder(never) => match *never {},
        }
    }
}

/// Async IPC listener.
pub struct AsyncIpcListener {
    #[cfg(unix)]
    inner: tokio::net::UnixListener,
    #[cfg(windows)]
    _private: (),
}

impl AsyncIpcListener {
    /// Bind at the given path. Creates parent dir with 0700 on Unix.
    pub fn bind(path: &Path) -> std::io::Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
                std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
            }
            let _ = std::fs::remove_file(path);
            let inner = tokio::net::UnixListener::bind(path)?;
            Ok(Self { inner })
        }
        #[cfg(windows)]
        {
            let _ = path;
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "async named pipe not yet implemented on Windows",
            ))
        }
    }

    /// Accept next connection.
    pub async fn accept(&self) -> std::io::Result<AsyncIpcStream> {
        #[cfg(unix)]
        {
            let (stream, _) = self.inner.accept().await?;
            Ok(AsyncIpcStream {
                inner: InnerStream::Unix(stream),
            })
        }
        #[cfg(windows)]
        {
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "async named pipe not yet implemented on Windows",
            ))
        }
    }
}

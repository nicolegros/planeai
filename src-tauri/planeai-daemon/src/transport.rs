use std::path::PathBuf;
use tokio::io::{AsyncRead, AsyncWrite};

/// Returns the default daemon socket path per platform conventions.
pub fn default_socket_path() -> PathBuf {
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

/// Platform-abstracted stream wrapping AsyncRead + AsyncWrite.
pub struct DaemonStream {
    inner: InnerStream,
}

enum InnerStream {
    #[cfg(unix)]
    Unix(tokio::net::UnixStream),
    #[cfg(windows)]
    Pipe(tokio::net::windows::named_pipe::NamedPipeServer),
}

impl AsyncRead for DaemonStream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match &mut self.get_mut().inner {
            #[cfg(unix)]
            InnerStream::Unix(s) => std::pin::Pin::new(s).poll_read(cx, buf),
            #[cfg(windows)]
            InnerStream::Pipe(s) => std::pin::Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for DaemonStream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        match &mut self.get_mut().inner {
            #[cfg(unix)]
            InnerStream::Unix(s) => std::pin::Pin::new(s).poll_write(cx, buf),
            #[cfg(windows)]
            InnerStream::Pipe(s) => std::pin::Pin::new(s).poll_write(cx, buf),
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
            InnerStream::Pipe(s) => std::pin::Pin::new(s).poll_flush(cx),
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
            InnerStream::Pipe(s) => std::pin::Pin::new(s).poll_shutdown(cx),
        }
    }
}

/// Platform-abstracted listener.
pub struct DaemonListener {
    #[cfg(unix)]
    inner: tokio::net::UnixListener,
}

impl DaemonListener {
    /// Bind on the given socket path. Creates parent directory with 0700 permissions on Unix.
    pub fn bind(path: &std::path::Path) -> std::io::Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
                std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
            }
            // Remove stale socket
            let _ = std::fs::remove_file(path);
            let listener = tokio::net::UnixListener::bind(path)?;
            Ok(Self { inner: listener })
        }
        #[cfg(windows)]
        {
            // On Windows we use named pipes; bind is a no-op placeholder,
            // actual pipe creation happens in accept().
            let _ = path;
            Ok(Self {})
        }
    }

    /// Accept next connection.
    pub async fn accept(&self) -> std::io::Result<DaemonStream> {
        #[cfg(unix)]
        {
            let (stream, _) = self.inner.accept().await?;
            Ok(DaemonStream {
                inner: InnerStream::Unix(stream),
            })
        }
        #[cfg(windows)]
        {
            // Simplified Windows named pipe implementation
            unimplemented!("Windows named pipe accept not yet implemented")
        }
    }
}

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
    NamedPipe(tokio::net::windows::named_pipe::NamedPipeClient),
    #[cfg(windows)]
    NamedPipeServer(tokio::net::windows::named_pipe::NamedPipeServer),
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
            InnerStream::NamedPipe(s) => std::pin::Pin::new(s).poll_read(cx, buf),
            #[cfg(windows)]
            InnerStream::NamedPipeServer(s) => std::pin::Pin::new(s).poll_read(cx, buf),
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
            InnerStream::NamedPipe(s) => std::pin::Pin::new(s).poll_write(cx, buf),
            #[cfg(windows)]
            InnerStream::NamedPipeServer(s) => std::pin::Pin::new(s).poll_write(cx, buf),
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
            InnerStream::NamedPipe(s) => std::pin::Pin::new(s).poll_flush(cx),
            #[cfg(windows)]
            InnerStream::NamedPipeServer(s) => std::pin::Pin::new(s).poll_flush(cx),
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
            InnerStream::NamedPipe(s) => std::pin::Pin::new(s).poll_shutdown(cx),
            #[cfg(windows)]
            InnerStream::NamedPipeServer(s) => std::pin::Pin::new(s).poll_shutdown(cx),
        }
    }
}

impl AsyncIpcStream {
    /// Connect to a Unix socket / named pipe at the given path.
    ///
    /// On Windows, retries with exponential backoff when the pipe returns
    /// ERROR_PIPE_BUSY (os error 231), which occurs transiently between
    /// the server accepting one connection and creating the next pipe instance.
    pub async fn connect(path: &Path) -> std::io::Result<Self> {
        #[cfg(unix)]
        {
            let stream = tokio::net::UnixStream::connect(path).await?;
            Ok(Self {
                inner: InnerStream::Unix(stream),
            })
        }
        #[cfg(windows)]
        {
            use tokio::net::windows::named_pipe::ClientOptions;

            const ERROR_PIPE_BUSY: i32 = 231;
            const RETRY_DELAYS_MS: &[u64] = &[10, 20, 50, 100, 200];

            let pipe_name = path.to_string_lossy();
            let opts = ClientOptions::new();

            for delay in RETRY_DELAYS_MS {
                match opts.open(&*pipe_name) {
                    Ok(client) => {
                        return Ok(Self {
                            inner: InnerStream::NamedPipe(client),
                        });
                    }
                    Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) => {
                        tokio::time::sleep(std::time::Duration::from_millis(*delay)).await;
                    }
                    Err(e) => return Err(e),
                }
            }

            // Final attempt after all retries exhausted
            let client = opts.open(&*pipe_name)?;
            Ok(Self {
                inner: InnerStream::NamedPipe(client),
            })
        }
    }
}

/// Async IPC listener.
///
/// On Windows, wraps a named pipe server. Each `accept()` creates a new pipe instance
/// and waits for a client to connect (standard multi-client named pipe pattern).
pub struct AsyncIpcListener {
    #[cfg(unix)]
    inner: tokio::net::UnixListener,
    #[cfg(windows)]
    pipe_name: String,
    #[cfg(windows)]
    is_first: std::sync::atomic::AtomicBool,
}

impl AsyncIpcListener {
    /// Bind at the given path. Creates parent dir with 0700 on Unix.
    /// On Windows, `path` is a named pipe address (e.g. `\\.\pipe\planeai-daemon`).
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
            let pipe_name = path.to_string_lossy().into_owned();
            Ok(Self {
                pipe_name,
                is_first: std::sync::atomic::AtomicBool::new(true),
            })
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
            use std::sync::atomic::Ordering;
            use tokio::net::windows::named_pipe::ServerOptions;

            let first = self.is_first.swap(false, Ordering::SeqCst);
            let server = ServerOptions::new()
                .first_pipe_instance(first)
                .create(&self.pipe_name)?;
            server.connect().await?;
            Ok(AsyncIpcStream {
                inner: InnerStream::NamedPipeServer(server),
            })
        }
    }
}

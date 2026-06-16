use crate::Channel;
use std::io::{Read, Write};
use std::path::Path;

/// Synchronous IPC stream (Unix socket or Windows named pipe).
pub struct IpcStream {
    #[cfg(unix)]
    inner: std::os::unix::net::UnixStream,
    #[cfg(windows)]
    inner: std::fs::File,
}

/// Synchronous IPC listener.
pub struct IpcListener {
    #[cfg(unix)]
    inner: std::os::unix::net::UnixListener,
    #[cfg(windows)]
    pipe_name: Vec<u16>,
    #[cfg(windows)]
    access: u32,
}

impl IpcListener {
    pub fn bind(channel: Channel, app_dir: &Path) -> Result<Self, String> {
        let path = crate::socket_path(channel, app_dir);
        #[cfg(unix)]
        {
            let _ = std::fs::remove_file(&path);
            let inner = std::os::unix::net::UnixListener::bind(&path)
                .map_err(|e| format!("bind failed: {e}"))?;
            Ok(Self { inner })
        }
        #[cfg(windows)]
        {
            use windows_sys::Win32::Storage::FileSystem::{
                PIPE_ACCESS_DUPLEX, PIPE_ACCESS_INBOUND,
            };
            let access = match channel {
                Channel::Notify => PIPE_ACCESS_INBOUND,
                _ => PIPE_ACCESS_DUPLEX,
            };
            let name = path.to_string_lossy().to_string();
            let pipe_name: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
            Ok(Self { pipe_name, access })
        }
    }

    pub fn accept(&self) -> Result<IpcStream, String> {
        #[cfg(unix)]
        {
            let (stream, _) = self
                .inner
                .accept()
                .map_err(|e| format!("accept failed: {e}"))?;
            Ok(IpcStream { inner: stream })
        }
        #[cfg(windows)]
        {
            use std::os::windows::io::FromRawHandle;
            use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
            use windows_sys::Win32::System::Pipes::*;
            let handle = unsafe {
                CreateNamedPipeW(
                    self.pipe_name.as_ptr(),
                    self.access,
                    PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                    PIPE_UNLIMITED_INSTANCES,
                    512,
                    512,
                    0,
                    std::ptr::null(),
                )
            };
            if handle == INVALID_HANDLE_VALUE {
                return Err("failed to create named pipe".to_string());
            }
            unsafe { ConnectNamedPipe(handle, std::ptr::null_mut()) };
            let file = unsafe { std::fs::File::from_raw_handle(handle as *mut _) };
            Ok(IpcStream { inner: file })
        }
    }
}

/// Connect to an IPC channel.
pub fn connect(channel: Channel, app_dir: &Path) -> Result<IpcStream, String> {
    let path = crate::socket_path(channel, app_dir);
    #[cfg(unix)]
    {
        let inner = std::os::unix::net::UnixStream::connect(&path)
            .map_err(|e| format!("connect failed: {e}"))?;
        Ok(IpcStream { inner })
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::FromRawHandle;
        use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::Storage::FileSystem::{CreateFileW, OPEN_EXISTING};
        let wide: Vec<u16> = path
            .to_string_lossy()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                0,
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err("failed to connect to pipe".to_string());
        }
        let file = unsafe { std::fs::File::from_raw_handle(handle as *mut _) };
        Ok(IpcStream { inner: file })
    }
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

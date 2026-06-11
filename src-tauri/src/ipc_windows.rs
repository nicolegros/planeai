use std::io::{Read, Write};
use std::os::windows::io::FromRawHandle;
use std::path::Path;

use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, GENERIC_READ, GENERIC_WRITE, OPEN_EXISTING, PIPE_ACCESS_DUPLEX,
    PIPE_ACCESS_INBOUND,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE,
    PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};

use crate::ipc::Channel;

const PIPE_PREFIX: &str = r"\\.\pipe\planeai-";

pub struct IpcListener {
    pipe_name: Vec<u16>,
    access: u32,
}

pub struct IpcStream {
    inner: std::fs::File,
}

impl IpcListener {
    pub fn bind(channel: Channel, _app_dir: &Path) -> Result<Self, String> {
        let access = match channel {
            Channel::Notify => PIPE_ACCESS_INBOUND,
            Channel::Symphony => PIPE_ACCESS_DUPLEX,
        };
        let name = pipe_name(channel);
        let pipe_name: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        Ok(Self { pipe_name, access })
    }

    pub fn accept(&self) -> Result<IpcStream, String> {
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

pub fn connect(channel: Channel, _app_dir: &Path) -> Result<IpcStream, String> {
    let name = pipe_name(channel);
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
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

pub fn channel_exists(_channel: Channel, _app_dir: &Path) -> bool {
    // Named pipes don't have filesystem presence; assume available.
    true
}

fn pipe_name(channel: Channel) -> String {
    format!("{}{}", PIPE_PREFIX, channel.socket_name())
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

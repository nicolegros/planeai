//! Cross-platform IPC abstraction over Unix sockets / Windows named pipes.

pub use crate::ipc_platform::{address, channel_exists, connect, IpcListener, IpcStream};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Notify,
    Symphony,
}

impl Channel {
    /// Returns the base name used for socket files (Unix) or pipe suffix (Windows).
    pub fn name(&self) -> &'static str {
        match self {
            Channel::Notify => "notify",
            Channel::Symphony => "symphony",
        }
    }
}

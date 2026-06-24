//! Cross-platform IPC abstraction — delegates to `planeai-ipc` crate.

pub use planeai_ipc::{address, channel_exists, connect, Channel, IpcListener, IpcStream};

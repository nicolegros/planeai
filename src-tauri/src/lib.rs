pub mod cleanup;
pub mod cli;
pub mod command;
pub mod config;
pub mod db;
pub mod git;
pub mod ipc;
#[cfg(unix)]
#[path = "ipc_unix.rs"]
mod ipc_platform;
#[cfg(windows)]
#[path = "ipc_windows.rs"]
mod ipc_platform;
pub mod logging;
pub mod paths;
pub mod session_ops;
pub mod task_cli;
pub mod template;
#[cfg(not(windows))]
pub mod tmux;

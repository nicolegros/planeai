pub mod agent_hooks;
pub mod background_work;
pub mod capture_cursor;
pub mod cleanup;
pub mod command;
pub mod dispatch;
pub mod git;
pub mod jsonc;
pub mod mcp;
pub mod notify;
pub mod orchestrator;
pub mod prompt_lock;
pub mod services;
pub mod session;
pub mod session_launch;
pub mod task;
pub mod task_lifecycle;
pub mod template;
pub mod text;

// One source for PlaneAI's data folder, shared with the app and the daemon.
pub use planeai_paths::{app_data_dir, notify_socket_path};

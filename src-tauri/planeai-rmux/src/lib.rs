//! rmux-backed persistent session hosting for PlaneAI.
//!
//! rmux is used as a **process host, not a user interface**: the pane output
//! stream is byte-exactly the child process's own output, so PlaneAI keeps
//! rendering in xterm. See `docs/adr/0012-rmux-candidate-session-backend.md` for
//! the mapping, ownership rules, and the probe evidence behind them.
//!
//! This crate owns the daemon connection, session/pane lifecycle, and the
//! bounded output buffering that reconciles rmux's drop-on-slow-consumer
//! behaviour with PlaneAI's `pause()`/`resume()` contract. It does not depend on
//! Tauri.

mod client;
mod config;
mod error;
mod naming;
mod output;

pub use client::{shell_argv, AttachedPane, LivePane, ResourceHandle, ResourceSpawn, RmuxClient};
pub use config::{Endpoint, RmuxConfig, DAEMON_BINARY_ENV, ENDPOINT_ENV};
pub use error::{Error, Result};
pub use naming::{is_planeai_session, WorkspaceKey, WorkspaceName, SESSION_PREFIX};
pub use output::{Gap, GapCause, OutputBuffer};

/// Backend identifier stored in the sessions table.
pub const BACKEND: &str = "rmux";

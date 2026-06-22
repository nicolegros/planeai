//! planeai-pty: Shared PTY/session I/O infrastructure for PlaneAI.
//!
//! This crate owns local PTY spawn, reader thread, coalescing/flushing,
//! push-based output events, write, resize, kill/exit detection, and
//! pipeline diagnostics.
//!
//! It is NOT a terminal emulator and does not render or parse ANSI.

pub mod config;
pub mod diagnostics;
pub mod error;
pub mod event;
pub(crate) mod flow_control;
pub mod local;

#[cfg(not(windows))]
#[path = "platform_unix.rs"]
pub(crate) mod platform;

#[cfg(windows)]
#[path = "platform_windows.rs"]
pub(crate) mod platform;

pub use config::{LocalPtyConfig, QueuePolicy};
pub use diagnostics::{DiagnosticsSnapshot, PipelineDiagnostics};
pub use event::{PtyEvent, PtyEventSink, SessionId};
pub use local::LocalPtySession;
